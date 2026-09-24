//! Sine and cosine computed without calling `sin` or `cos`.
//!
//! # Why this module exists, when `sin` is one instruction
//!
//! Topic 44 states this workspace's shading rule: **no
//! transcendental function may reach a colour**, because the four backends'
//! implementations of them differ in the last place and this engine blesses one
//! set of golden images across all four. [`crate::fog::exp_neg`] answered that
//! rule for the exponential by building it out of the operations IEEE-754 pins
//! down, and `docs/plan/55-water.md` decision 6 takes the same answer for
//! trigonometry: every published water model is written in sines and cosines,
//! and a construction lets Gerstner waves, a sum of sines and an FFT's phase
//! rotation be written the way their sources write them rather than through a
//! baked table.
//!
//! # The construction
//!
//! It is the one `fdlibm` uses, narrowed to `f32` and to a bounded domain. Each
//! step is an operation IEEE-754 specifies exactly, so no part of it is a
//! vendor's choice:
//!
//! 1. **Range reduction.** `n = round(x * 2/π)` and `r = x - n * π/2`, which
//!    leaves `|r|` at a quarter turn or less, with `n` kept as an integer.
//!    `π/2` is spent in **three** parts — [`PIO2_HI`], [`PIO2_MID`] and
//!    [`PIO2_LO`] — where [`crate::fog`] spends `ln 2` in two. The first two
//!    have their low mantissa bits cleared so their products with `n` are
//!    exact, and each subtraction then cancels exactly where `x` sits near a
//!    multiple of `π/2`, which is precisely where the result is smallest and a
//!    lost bit is the largest relative error. Two parts are not enough in
//!    `f32`: measured on 2026-09-15, a two-part split was hundreds of ulp wrong
//!    at the `f32` nearest `3π/2`, well inside one turn either side of zero,
//!    because the pair holds too few bits of `π/2` to cancel against.
//!    `fdlibm`'s `__rem_pio2` splits it in three for the same reason.
//! 2. **The kernel.** The Taylor series of `sin r` and `cos r` in Horner form
//!    over `-r²`, the sine taking the odd terms of [`KERNEL_DEGREE`] and the
//!    cosine the even ones. The coefficients are the reciprocal factorials —
//!    exact rationals rather than a fit, so there is no published digit to
//!    transcribe wrongly, and `the_kernel_coefficients_are_reciprocal_factorials`
//!    checks each one against the factorial it claims to be.
//! 3. **The quadrant.** `n mod 4` picks which kernel answers and with which
//!    sign, so the function is read off a quarter turn it can evaluate well.
//!
//! Measured against `f64::sin` and `f64::cos` over every `f32` in the domain it
//! is within [`MAX_KERNEL_ULP`] units in the last place — see
//! `the_sine_tracks_the_real_one` and its exhaustive counterpart.
//!
//! # What is not claimed
//!
//! **Not bit-identical output across backends.** On the CPU it is: Rust never
//! contracts a multiply and an add on its own, so every target runs the same
//! operations in the same order. A shader compiler may still fuse them into an
//! FMA, which is why `docs/plan/55-water.md` states the GPU copy as equal within
//! a known bound rather than bit for bit — the same freedom
//! [`crate::fog::exp_neg`] runs under.
//!
//! **Not correctly rounded.** The result can land a step or so from the nearest
//! `f32` to the true value, and [`MAX_KERNEL_ULP`] is the measured ceiling on
//! how far. Carrying the reduction's rounding error as a second term, as
//! `fdlibm` does, was measured and moved that ceiling by less than a tenth of a
//! unit, so it is not paid for.
//!
//! **Not the whole real line.** Arguments past [`MAX_ARGUMENT`] saturate; see
//! [`sin`]. The expected caller hands over a phase it has already reduced.
//!
//! **No shader carries a copy yet.** The Slang mirror lands with its first
//! shader caller, held to these constants by the guard
//! [`crate::fog`]'s `the_shader_spells_the_same_constants` is for the
//! exponential.

/// `2/π`. Turns an angle into quarter turns, so the nearest whole number of
/// them can be taken out of it.
///
/// Named here rather than used from `core` at the call site because a shader
/// mirror has to spell it as a literal, and a named constant is what a guard
/// can hold that literal against.
pub const FRAC_2_PI: f32 = core::f32::consts::FRAC_2_PI;

/// The leading part of `π/2`: its leading bits, truncated, with enough of the
/// low mantissa bits cleared that its product with every `n` [`MAX_ARGUMENT`]
/// admits still fits a binary32 significand, and so is exact.
///
/// `every_product_the_domain_admits_is_exact` checks that for every `n` rather
/// than trusting the bit count.
pub const PIO2_HI: f32 = 1.570_793_2;

/// The bits of `π/2` that follow [`PIO2_HI`]'s, as many of them and cleared
/// the same way, for the same reason.
pub const PIO2_MID: f32 = 3.174_936_9e-6;

/// What [`PIO2_HI`] and [`PIO2_MID`] leave of `π/2`, rounded to an `f32`.
///
/// Its product with `n` rounds, which is harmless: by the time it is
/// subtracted the cancellation has already happened, and this last part is
/// small beside what remains.
pub const PIO2_LO: f32 = 2.563_344e-12;

/// The highest power in the Taylor series the kernel evaluates. The sine takes
/// the odd terms up to one below it and the cosine the even terms up to it.
///
/// Measured over every `f32` in the domain: one term more in either series
/// leaves the worst error unchanged, and one fewer in either raises it past
/// [`MAX_KERNEL_ULP`].
pub const KERNEL_DEGREE: usize = 10;

/// Units in the last place [`sin`] and [`cos`] are allowed to differ from
/// `f64::sin` and `f64::cos`.
///
/// Measured rather than chosen, over every `f32` in the domain by
/// `every_f32_in_the_domain_tracks_the_real_one`.
pub const MAX_KERNEL_ULP: f64 = 1.5;

/// The largest magnitude [`sin`] and [`cos`] accept before they saturate.
///
/// Chosen so `round(x * 2/π)` stays within the bits [`PIO2_HI`] and
/// [`PIO2_MID`] leave room for, which is many turns either side of zero: far
/// more than a phase that has been reduced, as the ocean's is, can reach. A
/// wider domain costs accuracy rather than only constants — the same
/// construction with two more bits cleared, for four times the domain,
/// measured a worst case past [`MAX_KERNEL_ULP`].
pub const MAX_ARGUMENT: f32 = 100.0;

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
    2.480_158_7e-5,
    2.755_731_9e-6,
    2.755_732e-7,
];

// The sine's series ends one below the degree and the cosine's on it, which
// are the odd and even powers they need only while the degree is even.
const _: () = assert!(
    KERNEL_DEGREE.is_multiple_of(2),
    "KERNEL_DEGREE must be even"
);

/// `x` as a whole number of quarter turns and what is left over.
///
/// The count goes through an integer before it multiplies anything, which is
/// also what makes a zero count `+0.0`: `x - (+0.0)` keeps the sign of a zero
/// `x`, where subtracting the `-0.0` that `round` gives for one would not.
fn reduce(x: f32) -> (i32, f32) {
    let clamped = x.clamp(-MAX_ARGUMENT, MAX_ARGUMENT);
    let quadrant = (clamped * FRAC_2_PI).round() as i32;
    let n = quadrant as f32;
    let reduced = ((clamped - n * PIO2_HI) - n * PIO2_MID) - n * PIO2_LO;
    (quadrant, reduced)
}

/// `sin r` for `|r|` at a quarter turn or a hair past it.
///
/// Written as `r` plus a correction rather than `r` times a series, because the
/// correction is small beside `r` and so rounds once, at the end, where the
/// product form rounds the series first and measured half a unit worse.
///
/// The `copysign` is for a zero `r` alone. No product can carry a zero's sign
/// into that correction and still give every other `r` the opposite one, so
/// the sum turns `-0.0` into `+0.0`; everywhere else the result already has
/// `r`'s sign, and the copy changes nothing.
fn sin_kernel(r: f32) -> f32 {
    let w = -(r * r);
    let mut series = KERNEL[KERNEL_DEGREE - 1];
    for coefficient in KERNEL[3..KERNEL_DEGREE - 1].iter().step_by(2).rev() {
        series = series * w + coefficient;
    }
    (r + r * w * series).copysign(r)
}

/// `cos r` for `|r|` at a quarter turn or a hair past it.
fn cos_kernel(r: f32) -> f32 {
    let w = -(r * r);
    let mut series = KERNEL[KERNEL_DEGREE];
    for coefficient in KERNEL[2..KERNEL_DEGREE].iter().step_by(2).rev() {
        series = series * w + coefficient;
    }
    1.0 + w * series
}

/// `sin x` for any `x`, using only operations IEEE-754 specifies exactly.
///
/// Saturates rather than wrapping or failing: an `x` past [`MAX_ARGUMENT`],
/// infinities included, returns what that argument returns, and one past its
/// negation what *its* negation does. That keeps the result finite and
/// continuous — a caller that forgot to reduce its phase sees a wave stop
/// moving rather than a `NaN` spreading through a frame. A `NaN` argument
/// returns `NaN`, and nothing in this module produces one.
///
/// `sin(0.0)` is exactly `0.0` and `sin(-0.0)` exactly `-0.0`, and on the CPU
/// `sin(-x)` is `-sin(x)` bit for bit: every step of the construction is odd
/// or even in `x`, `round` included.
#[must_use]
pub fn sin(x: f32) -> f32 {
    let (quadrant, r) = reduce(x);
    match quadrant & 3 {
        0 => sin_kernel(r),
        1 => cos_kernel(r),
        2 => -sin_kernel(r),
        _ => -cos_kernel(r),
    }
}

/// `cos x` for any `x`, on [`sin`]'s terms: the same saturation past
/// [`MAX_ARGUMENT`], the same `NaN`, and the same construction.
///
/// `cos(0.0)` is exactly `1.0`, and on the CPU `cos(-x)` is `cos(x)` bit for
/// bit.
#[must_use]
pub fn cos(x: f32) -> f32 {
    let (quadrant, r) = reduce(x);
    match quadrant & 3 {
        0 => cos_kernel(r),
        1 => -sin_kernel(r),
        2 => -cos_kernel(r),
        _ => sin_kernel(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `π/2` to 127 bits past the binary point, truncated.
    ///
    /// Written out rather than derived because no type in `std` holds that many
    /// bits of it; `the_three_parts_sum_to_a_half_pi` checks the leading ones
    /// against `f64`'s, so a slip in the digits that matter most cannot pass.
    const HALF_PI_FIXED: u128 = 0xc90f_daa2_2168_c234_c4c6_628b_80dc_1cd1;

    /// The binary point of [`HALF_PI_FIXED`].
    const FIXED_POINT: i32 = 127;

    /// Uniform samples across the domain in [`for_each_sampled_argument`].
    const UNIFORM_STEPS: u32 = 2_000_000;

    /// `f32`s visited either side of each multiple of `π/4` in
    /// [`for_each_sampled_argument`].
    const NEIGHBOURS: u32 = 4096;

    /// `f64::sin` and `f64::cos` are the oracle throughout: a test is not
    /// shading, so the rule this module exists to satisfy does not bind here.
    ///
    /// How far `got` is from `want`, in units of the last place: the spacing of
    /// the `f32`s around `want`, so a correctly rounded result scores half a
    /// unit or less wherever it sits. [`crate::fog`]'s tests measure relative to
    /// half an epsilon instead, which counts one step as anything from one unit
    /// to two depending on where the value sits between powers of two; a bound
    /// that counts steps says more about the result than one that does not.
    fn ulps(got: f32, want: f64) -> f64 {
        #[expect(clippy::cast_possible_truncation, reason = "rounding is the point")]
        let mut below = want.abs() as f32;
        if f64::from(below) > want.abs() {
            below = below.next_down();
        }
        let spacing = below.next_up() - below;
        (f64::from(got) - want).abs() / f64::from(spacing)
    }

    /// Every argument the sampled sweeps visit: a fine uniform step across the
    /// whole domain, both edges and their neighbours, and a run of consecutive
    /// `f32`s around every multiple of `π/4` inside it.
    ///
    /// The multiples are where the construction can fail and a uniform step
    /// will not land: `π/2`'s multiples are where the reduction cancels to
    /// almost nothing, and the odd multiples of `π/4` are where `round` changes
    /// its mind and the kernel runs at the end of its interval.
    fn for_each_sampled_argument(mut visit: impl FnMut(f32)) {
        let span = 2.0 * f64::from(MAX_ARGUMENT);
        for step in 0..=UNIFORM_STEPS {
            let x = -f64::from(MAX_ARGUMENT) + span * f64::from(step) / f64::from(UNIFORM_STEPS);
            #[expect(clippy::cast_possible_truncation, reason = "sampling in f32")]
            visit(x as f32);
        }

        for edge in [MAX_ARGUMENT, -MAX_ARGUMENT] {
            let mut x = edge;
            for _ in 0..NEIGHBOURS {
                visit(x);
                x = if edge > 0.0 {
                    x.next_down()
                } else {
                    x.next_up()
                };
            }
        }

        #[expect(clippy::cast_possible_truncation, reason = "a small count")]
        let multiples = (f64::from(MAX_ARGUMENT) / core::f64::consts::FRAC_PI_4) as i32;
        for multiple in -multiples..=multiples {
            #[expect(clippy::cast_possible_truncation, reason = "the nearest f32")]
            let centre = (f64::from(multiple) * core::f64::consts::FRAC_PI_4) as f32;
            let (mut up, mut down) = (centre, centre.next_down());
            for _ in 0..NEIGHBOURS {
                visit(up);
                visit(down);
                up = up.next_up();
                down = down.next_down();
            }
        }
    }

    /// The worst error `function` makes against `oracle` over the sampled
    /// arguments, where it makes it, and how many arguments were visited.
    fn worst_over_samples(function: fn(f32) -> f32, oracle: fn(f64) -> f64) -> (f64, f32, u32) {
        let (mut worst, mut worst_at, mut visited) = (0.0f64, 0.0f32, 0u32);
        for_each_sampled_argument(|x| {
            let error = ulps(function(x), oracle(f64::from(x)));
            if error > worst {
                worst = error;
                worst_at = x;
            }
            visited += 1;
        });
        (worst, worst_at, visited)
    }

    /// What one thread of the exhaustive sweep found: each function's worst
    /// error and the argument it made it at, and how many arguments it visited.
    struct Sweep {
        sin: (f64, f32),
        cos: (f64, f32),
        visited: u64,
    }

    /// An `f32`'s exact value in [`FIXED_POINT`] fixed point.
    fn fixed(value: f32) -> u128 {
        let bits = value.to_bits();
        assert!(
            value.is_normal() && value > 0.0,
            "{value} is not a positive normal"
        );
        let exponent = ((bits >> 23) & 0xff) as i32 - 127;
        let significand = u128::from((bits & 0x7f_ffff) | 0x80_0000);
        significand << (exponent - 23 + FIXED_POINT)
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
    fn the_three_parts_sum_to_a_half_pi() {
        // The literal's leading bits are `f64`'s, so the digits the reduction
        // leans on hardest are checked against something independent.
        let leading = HALF_PI_FIXED >> (FIXED_POINT - 52);
        let from_f64 =
            u128::from(core::f64::consts::FRAC_PI_2.to_bits() & 0xf_ffff_ffff_ffff) | (1u128 << 52);
        assert_eq!(leading, from_f64, "HALF_PI_FIXED is not π/2");

        let sum = fixed(PIO2_HI) + fixed(PIO2_MID) + fixed(PIO2_LO);
        let gap = HALF_PI_FIXED.abs_diff(sum);
        // Rounding PIO2_LO to an f32 is the only error the split is allowed;
        // the one more is HALF_PI_FIXED's own truncation.
        let last_place = fixed(PIO2_LO.next_up()) - fixed(PIO2_LO);
        assert!(
            gap <= last_place / 2 + 1,
            "the parts of π/2 miss it by {gap} in 2^-{FIXED_POINT}, more than \
             half PIO2_LO's last place, {}",
            last_place / 2
        );
    }

    #[test]
    fn every_product_the_domain_admits_is_exact() {
        let largest = (MAX_ARGUMENT * FRAC_2_PI).round() as i32;
        assert!(largest > 0, "the domain admits no quadrant but zero");
        for n in -largest..=largest {
            for (part, name) in [(PIO2_HI, "PIO2_HI"), (PIO2_MID, "PIO2_MID")] {
                let got = f64::from(n as f32 * part);
                let want = f64::from(n) * f64::from(part);
                assert_eq!(
                    got, want,
                    "{n} * {name} rounds, so the reduction is not exact"
                );
            }
        }
    }

    #[test]
    fn the_sine_tracks_the_real_one() {
        let (worst, worst_at, visited) = worst_over_samples(sin, f64::sin);
        assert!(
            visited > UNIFORM_STEPS,
            "the sweep covered only {visited} points"
        );
        assert!(
            worst <= MAX_KERNEL_ULP,
            "sin is {worst} ulp from f64::sin at x = {worst_at:e}, over the \
             {MAX_KERNEL_ULP} this module documents"
        );
    }

    #[test]
    fn the_cosine_tracks_the_real_one() {
        let (worst, worst_at, visited) = worst_over_samples(cos, f64::cos);
        assert!(
            visited > UNIFORM_STEPS,
            "the sweep covered only {visited} points"
        );
        assert!(
            worst <= MAX_KERNEL_ULP,
            "cos is {worst} ulp from f64::cos at x = {worst_at:e}, over the \
             {MAX_KERNEL_ULP} this module documents"
        );
    }

    /// Every `f32` from zero to [`MAX_ARGUMENT`], both functions, both signs.
    ///
    /// The sampled sweeps above are what `cargo test` runs; this is what
    /// [`MAX_KERNEL_ULP`] was measured by. It visits over a billion arguments,
    /// so it runs on every core and wants a release build:
    ///
    /// ```text
    /// cargo test --release -p crcbl-shaders trig -- --ignored
    /// ```
    #[test]
    #[ignore = "exhaustive over the domain; run in release, see the doc comment"]
    fn every_f32_in_the_domain_tracks_the_real_one() {
        let top = MAX_ARGUMENT.to_bits();
        let threads = std::thread::available_parallelism().map_or(1, |count| count.get() as u32);
        let chunk = top / threads + 1;
        let results: Vec<Sweep> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|thread| {
                    scope.spawn(move || {
                        let mut sweep = Sweep {
                            sin: (0.0, 0.0),
                            cos: (0.0, 0.0),
                            visited: 0,
                        };
                        let end = ((thread + 1) * chunk).min(top + 1);
                        for bits in thread * chunk..end {
                            let x = f32::from_bits(bits);
                            let (s, c) = (sin(x), cos(x));
                            assert_eq!(
                                sin(-x).to_bits(),
                                (-s).to_bits(),
                                "sin is not odd at {x:e}"
                            );
                            assert_eq!(cos(-x).to_bits(), c.to_bits(), "cos is not even at {x:e}");
                            let error = ulps(s, f64::from(x).sin());
                            if error > sweep.sin.0 {
                                sweep.sin = (error, x);
                            }
                            let error = ulps(c, f64::from(x).cos());
                            if error > sweep.cos.0 {
                                sweep.cos = (error, x);
                            }
                            sweep.visited += 1;
                        }
                        sweep
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("a sweep thread"))
                .collect()
        });

        let visited: u64 = results.iter().map(|sweep| sweep.visited).sum();
        assert_eq!(visited, u64::from(top) + 1, "the sweep skipped arguments");
        let worst = |pick: fn(&Sweep) -> (f64, f32)| {
            results
                .iter()
                .map(pick)
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .expect("at least one thread")
        };
        let (sin_worst, sin_at) = worst(|sweep| sweep.sin);
        let (cos_worst, cos_at) = worst(|sweep| sweep.cos);
        println!(
            "sin worst {sin_worst} ulp at {sin_at:e}; cos worst {cos_worst} ulp at {cos_at:e}"
        );
        assert!(
            sin_worst <= MAX_KERNEL_ULP && cos_worst <= MAX_KERNEL_ULP,
            "sin is {sin_worst} ulp off at x = {sin_at:e} and cos {cos_worst} at \
             x = {cos_at:e}, against the {MAX_KERNEL_ULP} this module documents"
        );
    }

    #[test]
    fn zero_is_exact() {
        assert_eq!(sin(0.0).to_bits(), 0.0f32.to_bits());
        assert_eq!(
            sin(-0.0).to_bits(),
            (-0.0f32).to_bits(),
            "sin(-0.0) lost its sign"
        );
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(cos(-0.0), 1.0);
    }

    #[test]
    fn the_sine_is_odd_and_the_cosine_even() {
        let mut visited = 0u32;
        for_each_sampled_argument(|x| {
            assert_eq!(
                sin(-x).to_bits(),
                (-sin(x)).to_bits(),
                "sin is not odd at {x:e}"
            );
            assert_eq!(
                cos(-x).to_bits(),
                cos(x).to_bits(),
                "cos is not even at {x:e}"
            );
            visited += 1;
        });
        assert!(
            visited > UNIFORM_STEPS,
            "the sweep covered only {visited} points"
        );
    }

    #[test]
    fn a_nan_argument_is_a_nan_result() {
        assert!(sin(f32::NAN).is_nan(), "sin(NaN) is {}", sin(f32::NAN));
        assert!(cos(f32::NAN).is_nan(), "cos(NaN) is {}", cos(f32::NAN));
    }

    #[test]
    fn arguments_past_the_edge_saturate() {
        for past in [
            MAX_ARGUMENT.next_up(),
            MAX_ARGUMENT * 2.0,
            f32::MAX,
            f32::INFINITY,
        ] {
            assert_eq!(sin(past), sin(MAX_ARGUMENT), "sin({past}) did not saturate");
            assert_eq!(
                sin(-past),
                sin(-MAX_ARGUMENT),
                "sin({}) did not saturate",
                -past
            );
            assert_eq!(cos(past), cos(MAX_ARGUMENT), "cos({past}) did not saturate");
            assert_eq!(
                cos(-past),
                cos(-MAX_ARGUMENT),
                "cos({}) did not saturate",
                -past
            );
        }
        for end in [sin(MAX_ARGUMENT), cos(MAX_ARGUMENT)] {
            assert!(end.is_finite(), "{end} is not finite");
        }
    }
}
