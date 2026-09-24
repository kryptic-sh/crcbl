//! Sine and cosine in `f64`, computed without calling `sin` or `cos`.
//!
//! # Why this module exists
//!
//! A simulation that replays has to reach the same bits on every target it
//! runs on, and the platform's `sin` and `cos` do not: glibc, Apple's libm,
//! the MSVC runtime and the browser's `Math.sin` each round in their own way,
//! so one tick of a spinning body lands on a different last bit on each. The
//! user decided on 2026-09-17 — recorded in `docs/notes/simulation.md`, "What
//! the deleted 05-physics plan left behind" — that the simulation's
//! transcendentals are
//! **constructed in-engine**: no platform `libm` and no `libm` crate, but a
//! range reduction and a polynomial built out of the operations IEEE-754
//! specifies exactly.
//!
//! It lives in `crcbl-core` because that is the crate both of its intended
//! callers already depend on — `crcbl-phys` for integrating rotation and
//! `crcbl-audio` for oscillators — and neither needs `crcbl-shaders`, where the
//! `f32` construction this one is patterned on (`crcbl_shaders::trig`) lives.
//!
//! # The construction
//!
//! It is `fdlibm`'s, without the multi-precision path for huge arguments:
//!
//! 1. **Range reduction.** `n = round(x * 2/π)` and `r = x - n * π/2`, which
//!    leaves `|r|` at a quarter turn or a hair past it. `π/2` is spent in four
//!    parts: [`PIO2_HI`], [`PIO2_MID`] and [`PIO2_LO`] each hold the next 33
//!    bits of it, so their products with every `n` [`MAX_ARGUMENT`] admits fit
//!    a binary64 significand and are exact, and [`PIO2_TAIL`] is what is left,
//!    rounded. The first three are `fdlibm`'s `pio2_1`, `pio2_2` and `pio2_3`
//!    bit for bit, and its `pio2_3t` is the fourth. Each subtraction cancels
//!    exactly where `x` sits near a multiple of `π/2`, which is precisely where
//!    the result is smallest and a lost bit is the largest relative error.
//! 2. **The kernel.** The Taylor series of `sin r` and `cos r` in Horner form
//!    over `-r²`, the sine taking the odd terms of [`KERNEL_DEGREE`] and the
//!    cosine the even ones. The coefficients are the reciprocal factorials,
//!    divided out at compile time, so there is no digit to transcribe wrongly.
//! 3. **The quadrant.** `n mod 4` picks which kernel answers and with which
//!    sign.
//!
//! Measured against a double-double reference, which the tests check against
//! values `bc` computed to 130 digits, it is within [`MAX_KERNEL_ULP`] units in
//! the last place of the true value.
//!
//! # What is claimed, and what is not
//!
//! **Bit-identical on every target.** Rust never contracts a multiply and an
//! add into an FMA on its own, and every step here is `+`, `-`, `*`, `/`,
//! `round`, `clamp` or `copysign`, each of which IEEE-754 specifies exactly. A
//! digest of the outputs over a fixed set of arguments is pinned by
//! `the_outputs_hash_to_the_pinned_digest`, so a target that disagrees in one
//! bit fails it.
//!
//! **Not correctly rounded.** [`MAX_KERNEL_ULP`] is the measured ceiling on how
//! far from the true value a result can land.
//!
//! **Not the whole real line.** Arguments past [`MAX_ARGUMENT`] saturate; see
//! [`sin`].

/// `2/π`. Turns an angle into quarter turns, so the nearest whole number of
/// them can be taken out of it.
pub const FRAC_2_PI: f64 = core::f64::consts::FRAC_2_PI;

/// The first 33 bits of `π/2`, truncated: `fdlibm`'s `pio2_1`.
///
/// Its product with every quadrant count [`MAX_ARGUMENT`] admits fits a
/// binary64 significand, and `every_product_the_domain_admits_is_exact` checks
/// that for every count rather than trusting the bit arithmetic.
pub const PIO2_HI: f64 = f64::from_bits(0x3ff9_21fb_5440_0000);

/// The 33 bits of `π/2` after [`PIO2_HI`]'s: `fdlibm`'s `pio2_2`.
pub const PIO2_MID: f64 = f64::from_bits(0x3dd0_b461_1a60_0000);

/// The 33 bits of `π/2` after [`PIO2_MID`]'s: `fdlibm`'s `pio2_3`.
pub const PIO2_LO: f64 = f64::from_bits(0x3ba3_198a_2e00_0000);

/// What [`PIO2_HI`], [`PIO2_MID`] and [`PIO2_LO`] leave of `π/2`, rounded to
/// an `f64`: `fdlibm`'s `pio2_3t`.
///
/// Its product with `n` rounds, which is harmless: by the time it is
/// subtracted the cancellation has already happened.
pub const PIO2_TAIL: f64 = f64::from_bits(0x397b_839a_2520_49c1);

/// The highest power in the Taylor series the kernel evaluates. The sine takes
/// the odd terms up to one below it and the cosine the even terms up to it.
///
/// Measured on 2026-09-17: one term more in each series left the worst error
/// over the random sweep unchanged, and one fewer raised the sampled sweeps
/// past [`MAX_KERNEL_ULP`], to 1.07 units.
pub const KERNEL_DEGREE: usize = 18;

/// Units in the last place [`sin`] and [`cos`] are allowed to differ from the
/// true value.
///
/// Measured rather than chosen: `every_argument_sampled_tracks_the_reference`
/// drew 640 million arguments on 2026-09-17 and the worst was 0.815 units, for
/// the sine and the cosine alike. The sampled sweeps `cargo test` runs hold
/// every result to it.
pub const MAX_KERNEL_ULP: f64 = 0.82;

/// The largest magnitude [`sin`] and [`cos`] accept before they saturate.
///
/// Chosen so the quadrant count stays under `2^20`, which is what lets the
/// 33-bit parts of `π/2` multiply it exactly: a million radians is some
/// 160 000 turns, far more than a rotation step or a reduced phase reaches.
pub const MAX_ARGUMENT: f64 = 1_000_000.0;

/// Reciprocal factorials, `1/i!`, index `i`, as Horner consumes them.
///
/// Divided out here rather than written as literals: every `i!` up to
/// [`KERNEL_DEGREE`] is an integer an `f64` holds exactly, and one IEEE
/// division of `1` by it is the correctly rounded reciprocal, on every target
/// and at compile time alike.
const KERNEL: [f64; KERNEL_DEGREE + 1] = {
    let mut kernel = [0.0; KERNEL_DEGREE + 1];
    let mut factorial = 1.0;
    let mut i = 0;
    while i <= KERNEL_DEGREE {
        if i > 1 {
            factorial *= i as f64;
        }
        kernel[i] = 1.0 / factorial;
        i += 1;
    }
    kernel
};

// The sine's series ends one below the degree and the cosine's on it, which
// are the odd and even powers they need only while the degree is even. And
// `KERNEL_DEGREE!` has to be an integer an `f64` holds exactly, which holds up
// to 22! — the cap is 20 because the test that checks the table builds each
// factorial in a `u64`.
const _: () = assert!(
    KERNEL_DEGREE.is_multiple_of(2) && KERNEL_DEGREE <= 20,
    "KERNEL_DEGREE must be even, and 20 or less"
);

/// Knuth's two-sum: `a + b` as the rounded sum and the exact error of rounding
/// it, so the two add to `a + b` exactly.
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let sum = a + b;
    let b_part = sum - a;
    (sum, (a - (sum - b_part)) + (b - b_part))
}

/// `x` as a whole number of quarter turns and what is left over, the leftover
/// as a rounded head and the small tail rounding it lost.
///
/// `x - n * PIO2_HI` is exact: both are multiples of `x`'s last place inside
/// the domain, and their difference is under one. The next two parts go
/// through [`two_sum`], so their rounding error is carried rather than
/// dropped, and the tail part's product is added to it. Carrying the tail is
/// what `fdlibm` does and what the shader construction measured as not worth
/// it in `f32`; in `f64` dropping it measured over two units in the last
/// place, against [`MAX_KERNEL_ULP`] with it.
///
/// The count goes through an integer before it multiplies anything, which is
/// also what makes a zero count `+0.0`: `x - (+0.0)` keeps the sign of a zero
/// `x`, where subtracting the `-0.0` that `round` gives for one would not.
fn reduce(x: f64) -> (i32, f64, f64) {
    let clamped = x.clamp(-MAX_ARGUMENT, MAX_ARGUMENT);
    let quadrant = (clamped * FRAC_2_PI).round() as i32;
    let n = f64::from(quadrant);
    let head = clamped - n * PIO2_HI;
    let (head, mid_error) = two_sum(head, -(n * PIO2_MID));
    let (head, lo_error) = two_sum(head, -(n * PIO2_LO));
    (quadrant, head, (mid_error + lo_error) - n * PIO2_TAIL)
}

/// Horner over `w` of every other reciprocal factorial from `1/first!` up to
/// the kernel's degree: the part of a series past its leading terms.
fn series(first: usize, w: f64) -> f64 {
    let last = if (KERNEL_DEGREE - first).is_multiple_of(2) {
        KERNEL_DEGREE
    } else {
        KERNEL_DEGREE - 1
    };
    let mut sum = KERNEL[last];
    for coefficient in KERNEL[first..last].iter().step_by(2).rev() {
        sum = sum * w + coefficient;
    }
    sum
}

/// `sin(r + tail)` for `|r|` at a quarter turn or a hair past it and `tail`
/// a few units in `r`'s last place at most.
///
/// `musl`'s `__sin`, with its fitted coefficients replaced by the reciprocal
/// factorials: `r` plus a correction, so the result rounds once, at the end,
/// and the tail enters through `cos r ≈ 1 - r²/2`, which is all of `cos r` a
/// value that small can see.
///
/// The `copysign` is for a zero `r` alone: the sum turns `-0.0` into `+0.0`,
/// and everywhere else the result already has `r`'s sign.
fn sin_kernel(r: f64, tail: f64) -> f64 {
    let z = r * r;
    let v = z * r;
    let higher = series(5, -z);
    (r - (((z * (0.5 * tail - v * higher)) - tail) + v * KERNEL[3])).copysign(r)
}

/// `cos(r + tail)` for `|r|` at a quarter turn or a hair past it and `tail`
/// a few units in `r`'s last place at most.
///
/// `musl`'s `__cos`, with the reciprocal factorials: `1 - r²/2` is rounded
/// and its rounding error recovered exactly — `1 - hz` is within a factor of
/// two of `1`, so `(1 - w) - hz` is exact — and added back with the higher
/// terms, so the only rounding left is the final sum's.
fn cos_kernel(r: f64, tail: f64) -> f64 {
    let z = r * r;
    let higher = z * z * series(4, -z);
    let half_z = 0.5 * z;
    let w = 1.0 - half_z;
    w + (((1.0 - w) - half_z) + (higher - r * tail))
}

/// `sin x` for any `x`, using only operations IEEE-754 specifies exactly.
///
/// Saturates rather than wrapping or failing: an `x` past [`MAX_ARGUMENT`],
/// infinities included, returns what that argument returns, and one past its
/// negation what *its* negation does. A `NaN` argument returns `NaN`, and
/// nothing in this module produces one.
///
/// `sin(0.0)` is exactly `0.0` and `sin(-0.0)` exactly `-0.0`, and `sin(-x)`
/// is `-sin(x)` bit for bit: every step of the construction is odd or even in
/// `x`, `round` included.
#[must_use]
pub fn sin(x: f64) -> f64 {
    let (quadrant, r, tail) = reduce(x);
    match quadrant & 3 {
        0 => sin_kernel(r, tail),
        1 => cos_kernel(r, tail),
        2 => -sin_kernel(r, tail),
        _ => -cos_kernel(r, tail),
    }
}

/// `cos x` for any `x`, on [`sin`]'s terms: the same saturation past
/// [`MAX_ARGUMENT`], the same `NaN`, and the same construction.
///
/// `cos(0.0)` is exactly `1.0`, and `cos(-x)` is `cos(x)` bit for bit.
#[must_use]
pub fn cos(x: f64) -> f64 {
    let (quadrant, r, tail) = reduce(x);
    match quadrant & 3 {
        0 => cos_kernel(r, tail),
        1 => -sin_kernel(r, tail),
        2 => -cos_kernel(r, tail),
        _ => sin_kernel(r, tail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value held as a sum of two `f64`s, the second no larger than half the
    /// first's last place: about 106 bits, which is the reference every
    /// accuracy claim here is measured against.
    ///
    /// The reference has to be more precise than an `f64`, because the claim
    /// being checked is about an `f64`'s last place, and it has to be
    /// independent of the platform, because the platform's `sin` is what this
    /// module exists not to trust. It is built from Dekker's and Knuth's exact
    /// sum and product (Shewchuk, "Adaptive Precision Floating-Point
    /// Arithmetic", 1997) with no fused multiply, and
    /// `the_reference_reproduces_the_golden_table` holds it to `bc`.
    #[derive(Clone, Copy, Debug)]
    struct Dd {
        hi: f64,
        lo: f64,
    }

    /// Knuth's two-sum: `a + b` exactly, as the rounded sum and its error.
    fn two_sum(a: f64, b: f64) -> (f64, f64) {
        let s = a + b;
        let bb = s - a;
        (s, (a - (s - bb)) + (b - bb))
    }

    /// Dekker's fast two-sum, for `|a| >= |b|`.
    fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
        let s = a + b;
        (s, b - (s - a))
    }

    /// Dekker's split of `a` into halves whose products are exact.
    fn split(a: f64) -> (f64, f64) {
        /// `2^27 + 1`.
        const SPLITTER: f64 = 134_217_729.0;
        let t = SPLITTER * a;
        let hi = t - (t - a);
        (hi, a - hi)
    }

    /// Dekker's two-product: `a * b` exactly, as the rounded product and its
    /// error, with no fused multiply-add.
    fn two_prod(a: f64, b: f64) -> (f64, f64) {
        let p = a * b;
        let (ah, al) = split(a);
        let (bh, bl) = split(b);
        (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
    }

    impl Dd {
        const fn from(value: f64) -> Self {
            Self { hi: value, lo: 0.0 }
        }

        fn add(self, other: Self) -> Self {
            let (s, e) = two_sum(self.hi, other.hi);
            let (t, f) = two_sum(self.lo, other.lo);
            let (s, e) = quick_two_sum(s, e + t);
            let (hi, lo) = quick_two_sum(s, e + f);
            Self { hi, lo }
        }

        fn neg(self) -> Self {
            Self {
                hi: -self.hi,
                lo: -self.lo,
            }
        }

        fn mul(self, other: Self) -> Self {
            let (p, e) = two_prod(self.hi, other.hi);
            let (hi, lo) = quick_two_sum(p, e + (self.hi * other.lo + self.lo * other.hi));
            Self { hi, lo }
        }

        fn div_f64(self, divisor: f64) -> Self {
            let q = self.hi / divisor;
            let (p, e) = two_prod(q, divisor);
            let r = ((self.hi - p) - e + self.lo) / divisor;
            let (hi, lo) = quick_two_sum(q, r);
            Self { hi, lo }
        }
    }

    /// `π/2` in 33-bit parts, 165 bits of it, each part exact in an `f64` and
    /// exact again when multiplied by any quadrant count under `2^20`.
    ///
    /// Cut from `bc -l`'s `2*a(1)` at `scale=120` in base 16; the leading
    /// three are this module's own [`PIO2_HI`], [`PIO2_MID`] and [`PIO2_LO`],
    /// which `the_reduction_constants_are_half_pi` checks. Five parts reach
    /// past anything the domain can see: the `f64` in it that comes nearest a
    /// multiple of `π/2` is still `10^-22` of itself away, so its remainder is
    /// near 2^-54, and resolving that to a thousandth of its last place — with
    /// a count of up to 2^20 multiplying the error — needs `π/2` to about
    /// 2^-135. The fifth part ends at 2^-165.
    const HALF_PI_PARTS: [f64; 5] = [
        f64::from_bits(0x3ff9_21fb_5440_0000),
        f64::from_bits(0x3dd0_b461_1a60_0000),
        f64::from_bits(0x3ba3_198a_2e00_0000),
        f64::from_bits(0x397b_839a_2400_0000),
        f64::from_bits(0x37b2_049c_1110_0000),
    ];

    /// The quadrant count and remainder of `x`, to double-double precision.
    fn reference_reduce(x: f64) -> (i32, Dd) {
        let quadrant = (x * FRAC_2_PI).round() as i32;
        let n = f64::from(quadrant);
        let mut reduced = Dd::from(x);
        for part in HALF_PI_PARTS {
            let (product, error) = two_prod(n, part);
            assert_eq!(error, 0.0, "{n} * a part of π/2 is not exact");
            reduced = reduced.add(Dd::from(product).neg());
        }
        (quadrant, reduced)
    }

    /// Terms of the reference's Taylor series: at a quarter turn the last one
    /// is under `10^-33`, far past what the double-double can hold.
    const REFERENCE_TERMS: u32 = 32;

    /// `sin r` and `cos r` to double-double precision, for a reduced `r`.
    fn reference_kernels(r: Dd) -> (Dd, Dd) {
        let r2 = r.mul(r);
        let (mut sin_term, mut sin_sum) = (r, r);
        let (mut cos_term, mut cos_sum) = (Dd::from(1.0), Dd::from(1.0));
        for k in 1..=REFERENCE_TERMS {
            let k = f64::from(k);
            sin_term = sin_term.mul(r2).div_f64(2.0 * k * (2.0 * k + 1.0)).neg();
            sin_sum = sin_sum.add(sin_term);
            cos_term = cos_term.mul(r2).div_f64((2.0 * k - 1.0) * (2.0 * k)).neg();
            cos_sum = cos_sum.add(cos_term);
        }
        (sin_sum, cos_sum)
    }

    fn reference_sin(x: f64) -> Dd {
        let (quadrant, r) = reference_reduce(x);
        let (s, c) = reference_kernels(r);
        match quadrant & 3 {
            0 => s,
            1 => c,
            2 => s.neg(),
            _ => c.neg(),
        }
    }

    fn reference_cos(x: f64) -> Dd {
        let (quadrant, r) = reference_reduce(x);
        let (s, c) = reference_kernels(r);
        match quadrant & 3 {
            0 => c,
            1 => s.neg(),
            2 => c.neg(),
            _ => s,
        }
    }

    /// How far `got` is from `want`, in units of the last place: the spacing of
    /// the `f64`s around `want`, so a correctly rounded result scores half a
    /// unit or less wherever it sits.
    fn ulps(got: f64, want: Dd) -> f64 {
        let magnitude = want.hi.abs();
        // Below a power of two the spacing halves, and a `lo` pulling the true
        // value under `hi` puts it there.
        let below = if want.lo * want.hi.signum() < 0.0 {
            magnitude.next_down()
        } else {
            magnitude
        };
        let spacing = below.next_up() - below;
        // `got - hi` is exact: the two are within a few units of each other.
        ((got - want.hi) - want.lo).abs() / spacing
    }

    /// Uniform samples across the domain in [`for_each_sampled_argument`].
    const UNIFORM_STEPS: u32 = 400_000;

    /// `f64`s visited either side of each golden argument, and either side of
    /// each of the first multiples of `π/4`, in [`for_each_sampled_argument`].
    const NEIGHBOURS: u32 = 64;

    /// Multiples of `π/4` from zero whose neighbourhoods are visited.
    const MULTIPLES: i32 = 1024;

    /// Every argument the sampled sweeps visit: a uniform step across the whole
    /// domain, a run of consecutive `f64`s around every golden argument — which
    /// include the `f64`s in the domain nearest a multiple of `π/2` — and
    /// around the first multiples of `π/4`.
    ///
    /// The multiples are where the construction can fail and a uniform step
    /// will not land: `π/2`'s are where the reduction cancels to almost
    /// nothing, and the odd multiples of `π/4` are where `round` changes its
    /// mind and the kernel runs at the end of its interval.
    fn for_each_sampled_argument(mut visit: impl FnMut(f64)) {
        let span = 2.0 * MAX_ARGUMENT;
        for step in 0..=UNIFORM_STEPS {
            visit(-MAX_ARGUMENT + span * f64::from(step) / f64::from(UNIFORM_STEPS));
        }
        let mut around = |centre: f64| {
            let (mut up, mut down) = (centre, centre.next_down());
            for _ in 0..NEIGHBOURS {
                // Past the edge the functions saturate, which
                // `arguments_past_the_edge_saturate` covers and the reference
                // does not.
                for x in [up, down] {
                    if x.abs() <= MAX_ARGUMENT {
                        visit(x);
                    }
                }
                up = up.next_up();
                down = down.next_down();
            }
        };
        for (bits, _, _) in golden_rows() {
            around(f64::from_bits(bits));
        }
        for multiple in -MULTIPLES..=MULTIPLES {
            around(f64::from(multiple) * core::f64::consts::FRAC_PI_4);
        }
        around(MAX_ARGUMENT);
        around(-MAX_ARGUMENT);
    }

    /// The worst error `function` makes against `reference` over the sampled
    /// arguments, where it makes it, and how many arguments were visited.
    fn worst_over_samples(function: fn(f64) -> f64, reference: fn(f64) -> Dd) -> (f64, f64, u32) {
        let (mut worst, mut worst_at, mut visited) = (0.0f64, 0.0f64, 0u32);
        for_each_sampled_argument(|x| {
            let error = ulps(function(x), reference(x));
            if error > worst {
                worst = error;
                worst_at = x;
            }
            visited += 1;
        });
        (worst, worst_at, visited)
    }

    /// A correctly rounded `f64` from one of [`GOLDEN`]'s decimal strings: the
    /// standard library's parse rounds to nearest, and forty significant
    /// digits leave no argument in the table near a rounding midpoint.
    fn parsed(text: &str) -> f64 {
        text.parse().expect("a golden value is a decimal")
    }

    #[test]
    fn the_reduction_constants_are_half_pi() {
        // The leading part's leading bits against `core`'s own π/2, so the
        // digits the reduction leans on hardest are checked against something
        // independent of `bc`.
        let core_bits = core::f64::consts::FRAC_PI_2.to_bits();
        let hi_bits = PIO2_HI.to_bits();
        assert_eq!(
            hi_bits >> 19,
            core_bits >> 19,
            "PIO2_HI is not the leading bits of π/2"
        );
        assert_eq!(
            hi_bits & ((1 << 19) - 1),
            0,
            "PIO2_HI has more than 33 bits"
        );

        assert_eq!(PIO2_HI, HALF_PI_PARTS[0]);
        assert_eq!(PIO2_MID, HALF_PI_PARTS[1]);
        assert_eq!(PIO2_LO, HALF_PI_PARTS[2]);
        let tail = HALF_PI_PARTS[3..]
            .iter()
            .rev()
            .fold(Dd::from(0.0), |sum, part| sum.add(Dd::from(*part)));
        assert_eq!(
            PIO2_TAIL, tail.hi,
            "PIO2_TAIL is not the rest of π/2 rounded"
        );
    }

    #[test]
    fn every_product_the_domain_admits_is_exact() {
        let largest = (MAX_ARGUMENT * FRAC_2_PI).round() as i32;
        assert!(
            largest > 1 << 19,
            "the domain admits only {largest} quadrants"
        );
        for n in -largest..=largest {
            for (part, name) in [
                (PIO2_HI, "PIO2_HI"),
                (PIO2_MID, "PIO2_MID"),
                (PIO2_LO, "PIO2_LO"),
            ] {
                let (_, error) = two_prod(f64::from(n), part);
                assert_eq!(
                    error, 0.0,
                    "{n} * {name} rounds, so the reduction is not exact"
                );
            }
        }
    }

    #[test]
    fn the_kernel_coefficients_are_reciprocal_factorials() {
        // Against a factorial built by integer multiplication, so a slip in the
        // compile-time loop — an off-by-one index, a skipped term — cannot
        // agree with itself.
        let mut factorial: u64 = 1;
        for (i, coefficient) in KERNEL.iter().enumerate() {
            if i > 1 {
                factorial *= i as u64;
            }
            let exact = factorial as f64;
            assert_eq!(exact as u64, factorial, "{i}! is not exact in an f64");
            assert_eq!(*coefficient, 1.0 / exact, "KERNEL[{i}] is not 1/{i}!");
        }
    }

    #[test]
    fn the_reference_reproduces_the_golden_table() {
        let rows = golden_rows().count();
        assert!(rows > 100, "the golden table has {rows} rows");
        for (bits, sine, cosine) in golden_rows() {
            let x = f64::from_bits(bits);
            assert_eq!(reference_sin(x).hi, parsed(sine), "reference sin at {x:e}");
            assert_eq!(
                reference_cos(x).hi,
                parsed(cosine),
                "reference cos at {x:e}"
            );
        }
    }

    #[test]
    fn the_construction_matches_the_golden_table() {
        for (bits, sine, cosine) in golden_rows() {
            let x = f64::from_bits(bits);
            // The golden value is the true one rounded, so the construction's
            // distance from it can exceed its distance from the truth by half.
            let bound = MAX_KERNEL_ULP + 0.5;
            let error = ulps(sin(x), Dd::from(parsed(sine)));
            assert!(error <= bound, "sin({x:e}) is {error} ulp from bc's value");
            let error = ulps(cos(x), Dd::from(parsed(cosine)));
            assert!(error <= bound, "cos({x:e}) is {error} ulp from bc's value");
        }
    }

    #[test]
    fn the_sine_tracks_the_reference() {
        let (worst, worst_at, visited) = worst_over_samples(sin, reference_sin);
        assert!(
            visited > UNIFORM_STEPS,
            "the sweep covered only {visited} points"
        );
        assert!(
            worst <= MAX_KERNEL_ULP,
            "sin is {worst} ulp from the reference at x = {worst_at:e}, over the \
             {MAX_KERNEL_ULP} this module documents"
        );
    }

    #[test]
    fn the_cosine_tracks_the_reference() {
        let (worst, worst_at, visited) = worst_over_samples(cos, reference_cos);
        assert!(
            visited > UNIFORM_STEPS,
            "the sweep covered only {visited} points"
        );
        assert!(
            worst <= MAX_KERNEL_ULP,
            "cos is {worst} ulp from the reference at x = {worst_at:e}, over the \
             {MAX_KERNEL_ULP} this module documents"
        );
    }

    /// Arguments drawn at random across the domain, both functions, on every
    /// core: what [`MAX_KERNEL_ULP`] was measured by.
    ///
    /// Half the draws are uniform in value and half uniform in exponent, so the
    /// small arguments a uniform draw would never land on are covered too. It
    /// wants a release build:
    ///
    /// ```text
    /// cargo test --release -p crcbl-core trig -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "a long random sweep; run in release, see the doc comment"]
    fn every_argument_sampled_tracks_the_reference() {
        /// Draws per thread.
        const DRAWS: u64 = 20_000_000;

        let threads = std::thread::available_parallelism().map_or(1, |count| count.get() as u64);
        let results: Vec<(f64, f64, f64, f64)> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|thread| {
                    scope.spawn(move || {
                        let mut worst = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
                        for draw in 0..DRAWS {
                            let unit = crate::rand::hash_unit(thread, draw);
                            let sign = if draw & 2 == 0 { 1.0 } else { -1.0 };
                            let x = if draw & 1 == 0 {
                                sign * unit * MAX_ARGUMENT
                            } else {
                                // 2^-30 up to MAX_ARGUMENT, uniform in exponent,
                                // spelled as repeated halving so no transcendental
                                // is needed to draw it.
                                let mut x = MAX_ARGUMENT * (0.5 + 0.5 * unit);
                                for _ in 0..(crate::rand::hash_u64(thread, draw) % 50) {
                                    x *= 0.5;
                                }
                                sign * x
                            };
                            let error = ulps(sin(x), reference_sin(x));
                            if error > worst.0 {
                                worst.0 = error;
                                worst.1 = x;
                            }
                            let error = ulps(cos(x), reference_cos(x));
                            if error > worst.2 {
                                worst.2 = error;
                                worst.3 = x;
                            }
                        }
                        worst
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().expect("a sweep thread"))
                .collect()
        });
        let sin_worst = results
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .expect("a thread");
        let cos_worst = results
            .iter()
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .expect("a thread");
        println!(
            "{} draws: sin worst {} ulp at {:e}; cos worst {} ulp at {:e}",
            DRAWS * threads,
            sin_worst.0,
            sin_worst.1,
            cos_worst.2,
            cos_worst.3
        );
        assert!(
            sin_worst.0 <= MAX_KERNEL_ULP && cos_worst.2 <= MAX_KERNEL_ULP,
            "over the {MAX_KERNEL_ULP} ulp this module documents"
        );
    }

    #[test]
    fn zero_is_exact_and_tiny_arguments_are_themselves() {
        assert_eq!(sin(0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(
            sin(-0.0).to_bits(),
            (-0.0f64).to_bits(),
            "sin(-0.0) lost its sign"
        );
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(cos(-0.0), 1.0);
        // Below 2^-27 the cubic term is under half the last place of `x`.
        for x in [1e-9, 1e-30, 1e-300, f64::MIN_POSITIVE, 5e-324] {
            assert_eq!(sin(x), x, "sin({x:e})");
            assert_eq!(cos(x), 1.0, "cos({x:e})");
        }
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
        assert!(sin(f64::NAN).is_nan(), "sin(NaN) is {}", sin(f64::NAN));
        assert!(cos(f64::NAN).is_nan(), "cos(NaN) is {}", cos(f64::NAN));
    }

    #[test]
    fn arguments_past_the_edge_saturate() {
        for past in [
            MAX_ARGUMENT.next_up(),
            MAX_ARGUMENT * 2.0,
            f64::MAX,
            f64::INFINITY,
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
    }

    /// FNV-1a over the bits of both functions at every sampled argument.
    fn digest() -> u64 {
        /// FNV-1a's 64-bit offset basis and prime.
        const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = BASIS;
        for_each_sampled_argument(|x| {
            for value in [sin(x), cos(x)] {
                for byte in value.to_bits().to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
                }
            }
        });
        hash
    }

    /// **The same bits on every run and every target.**
    ///
    /// Run to run is the two digests agreeing. Target to target is the pinned
    /// constant: it was taken on x86-64 Linux, and CI runs this test on
    /// aarch64 macOS and x86-64 Windows too, so a target whose arithmetic
    /// differs in one bit of one output fails here rather than in a replay.
    #[test]
    fn the_outputs_hash_to_the_pinned_digest() {
        /// The digest on x86-64 Linux, 2026-09-17.
        const PINNED: u64 = 0x20be_321e_0066_f5de;
        let first = digest();
        assert_eq!(first, digest(), "two runs over the same arguments disagree");
        assert_eq!(
            first, PINNED,
            "the outputs hash to {first:#018x}, not the pinned {PINNED:#018x}"
        );
    }

    /// One row per argument: its bits in hex, then its sine and cosine from
    /// `bc -l` at `scale=130`, cut to forty significant digits.
    ///
    /// The arguments are the `f64`s nearest the multiples of `π/2` inside the
    /// domain that come closest to them — found by an exhaustive search over
    /// every multiple, in exact integer arithmetic — with their neighbours; the
    /// `f64`s nearest odd multiples of `π/4` for a spread of counts, with their
    /// neighbours; and a spread of ordinary arguments from `10^-30` to
    /// [`MAX_ARGUMENT`]. Some are negated.
    const GOLDEN: &str = "\
39b4484bfeebc2a0 1.000000000000000083336420607585985350931e-30 1.000000000000000000000000000000000000000e+0
3bc79ca10c924223 9.999999999999999451532714542095716517295e-21 1.000000000000000000000000000000000000000e+0
3e112e0be826d695 1.000000000000000062114924791113189721098e-9 9.999999999999999994999999999999999377601e-1
3ee4f8b588e368f1 9.999999999833334151364705766078233184488e-6 9.999999999500000000004166584863598865110e-1
3f50624dd2f1a9fc 9.999998333333416874831395573527063339607e-4 9.999995000000416666652569611243370898969e-1
3fb999999999999a 9.983341664682815783019686785861666773597e-2 9.950041652780257655413751988623452563481e-1
3fd0000000000000 2.474039592545229295968487048493891958934e-1 9.689124217106447841445954494941891998041e-1
3fe0000000000000 4.794255386042030002732879352155713880818e-1 8.775825618903727161162815826038296519916e-1
3fe8000000000000 6.816387600233341667332419527798939353384e-1 7.316888688738208863118387530000845438405e-1
3fe921fb54442d17 7.071067811865474242473200220287570151995e-1 7.071067811865476245543687021809268777779e-1
3fe921fb54442d18 7.071067811865475027519429562175167462615e-1 7.071067811865475460497457679921806695017e-1
3fe921fb54442d19 7.071067811865475812565658904062677615595e-1 7.071067811865474675451228338034257454614e-1
3ff0000000000000 8.414709848078965066525023216302989996226e-1 5.403023058681397174009366074429766037323e-1
3ff4000000000000 9.489846193555862143484908470360492503780e-1 3.153223623952686654475385524380380137280e-1
3ff921fb54442d18 9.999999999999999999999999999999981253003e-1 6.123233995736765886130329661375001464640e-17
4000000000000000 9.092974268256816953960198659117448427023e-1 -4.161468365471423869975682295007621897660e-1
4002d97c7f3321d1 7.071067811865479033660403165217552519500e-1 -7.071067811865471454356484076877397248767e-1
4002d97c7f3321d2 7.071067811865475893475485797668419415160e-1 -7.071067811865474594541401444428501717956e-1
4002d97c7f3321d3 7.071067811865472753290568430117891788582e-1 -7.071067811865477734726318811978211664907e-1
4004000000000000 5.984721441039564940518547021861622717036e-1 -8.011436155469337148335027904673516644286e-1
4008000000000000 1.411200080598672221007448028081102798469e-1 -9.899924966004454572715727947312613023937e-1
400921f9f01b866e 2.653589793352730077419785079371113475691e-6 -9.999999999964792306043009097337437036147e-1
400921fb54442d18 1.224646799147353177226065932274997997083e-16 -9.999999999999999999999999999999925012011e-1
400f6a7a2955385d -7.071067811865471021378455959130512710979e-1 -7.071067811865479466638431282963946446516e-1
400f6a7a2955385e -7.071067811865474161563373326681809461039e-1 -7.071067811865476326453513915415005623046e-1
400f6a7a2955385f -7.071067811865477301748290694231711688861e-1 -7.071067811865473186268596547864670277338e-1
4010000000000000 -7.568024953079282513726390945118290941359e-1 -6.536436208636119146391681830977503814241e-1
4012d97c7f3321d2 -9.999999999999999999999999999999831277024e-1 -1.836970198721029765839098898412491256012e-16
4014000000000000 -9.589242746631384688931544061559939733525e-1 2.836621854632262644666391715135573083344e-1
4015fdbbe9bba774 -7.071067811865483039801376768257667881633e-1 7.071067811865467448215510473830718107765e-1
4015fdbbe9bba775 -7.071067811865476759431542033161565318674e-1 7.071067811865473728585345208935090691864e-1
4015fdbbe9bba776 -7.071067811865470479061707298059884666760e-1 7.071067811865480008955179944033885187009e-1
4018000000000000 -2.794154981989258728115554466118947596280e-1 9.601702866503660205456522979229244054519e-1
401921fb54442d18 -2.449293598294706354452131864549977627406e-16 9.999999999999999999999999999999700048043e-1
401c000000000000 6.569865987187890903969990915936351779369e-1 7.539022543433046381411975217191820122183e-1
401c463abeccb2ba 7.071067811865467015237482356083588264591e-1 7.071067811865483472779404886003816503263e-1
401c463abeccb2bb 7.071067811865473295607317091188345410432e-1 7.071067811865477192409570150908098502044e-1
401c463abeccb2bc 7.071067811865479575977151826287524467317e-1 7.071067811865470912039735415806802411871e-1
401f6a7a2955385e 9.999999999999999999999999999999531325068e-1 3.061616997868382943065164830687454815420e-16
4021475cc9eedf00 7.071067811865483905757433003749938612634e-1 -7.071067811865466582259454238336431909160e-1
4021475cc9eedf01 7.071067811865471345017763533553693644724e-1 -7.071067811865479142999123708541137235368e-1
4021475cc9eedf02 7.071067811865458784278094063335136320994e-1 -7.071067811865491703738793178723530205755e-1
4022d97c7f3321d2 3.673940397442059531678197796824920524208e-16 -9.999999999999999999999999999999325108098e-1
4024000000000000 -5.440211108893698134047476618513772816836e-1 -8.390715290764524522588639478240648345199e-1
40246b9c347764a3 -7.071067811865466149281426120589249041471e-1 -7.071067811865484338735461121496034209748e-1
40246b9c347764a4 -7.071067811865478710021095590794723491160e-1 -7.071067811865471777995791651300558365319e-1
40246b9c347764a5 -7.071067811865491270760765060977885585029e-1 -7.071067811865459217256122181082770165071e-1
4025fdbbe9bba775 -9.999999999999999999999999999999081397133e-1 -4.286263797015736120291230762962372457927e-16
40278fdb9effea46 -7.071067811865484771713489239242103294604e-1 7.071067811865465716303398002842039661524e-1
40278fdb9effea47 -7.071067811865472210973819769047396573657e-1 7.071067811865478277043067473048283234695e-1
40278fdb9effea48 -7.071067811865459650234150298830377496890e-1 7.071067811865490837782736943232214452046e-1
402921fb54442d18 -4.898587196589412708904263729099808320730e-16 9.999999999999999999999999999998800192174e-1
402dd85a7410f58c 7.071067811865485637669545474734161927543e-1 -7.071067811865464850347341767347541364857e-1
402dd85a7410f58d 7.071067811865473076929876004540993453558e-1 -7.071067811865477411087011237555323184991e-1
402dd85a7410f58e 7.071067811865460516190206534325512623754e-1 -7.071067811865489971826680707740792649305e-1
402f6a7a2955385e 6.123233995736765886130329661374622650212e-16 -9.999999999999999999999999999998125300272e-1
4036000000000000 -8.851309290403875921690256815772332463289e-3 -9.999608263946371264541747392126937741360e-1
403858eb79a20baf -7.071067811865488235537714181209701532170e-1 7.071067811865462252479173060863410180668e-1
403858eb79a20bb0 -7.071067811865463114058375240810281710161e-1 7.071067811865487373958512001265890946894e-1
403858eb79a20bb1 -7.071067811865437992579036300321612464872e-1 7.071067811865512495437850941579122289842e-1
403921fb54442d18 -9.797174393178825417808527458198441168810e-16 9.999999999999999999999999999995200768695e-1
4040800000000000 9.999118601072671457280843767078283143509e-1 -1.327674722305947891522003137100703896077e-2
4059000000000000 -5.063656411097587936565576104597854320650e-1 8.623188722876839341019385139508425355101e-1
4063896a5e80ff0d -7.071067811865650213088770771779333698784e-1 7.071067811865300274928116465988148362667e-1
4063896a5e80ff0e -7.071067811865449241254059250688670161271e-1 7.071067811865501246762827991312689531041e-1
4063896a5e80ff0f -7.071067811865248269419347723886043533891e-1 7.071067811865702218597539510925267609548e-1
4063a28c59d5433b 9.821933618642359725809130144060867880065e-16 9.999999999999999999999999999995176481000e-1
4076300000000000 -3.014435335948844921433028000865009959026e-5 -9.999999995456589801659358416927540811238e-1
40934a456d5cfaad 7.803344920002026620246792282697681381766e-2 -9.969507414140118169114399103023850225710e-1
4098880b30e00b83 -7.071067811867195481363667795114896213569e-1 7.071067811863755006653219028485665496112e-1
4098880b30e00b84 -7.071067811865587706685975817807948606435e-1 7.071067811865362781330911422500355536042e-1
4098880b30e00b85 -7.071067811863979932008283474935363247865e-1 7.071067811866970556008603450949407824547e-1
40988b2f704a940a 6.666535247945037459549907150093265139860e-14 9.999999999999999999999999977778653893953e-1
40b9213244698af6 -7.071067811871048129272612391988120960490e-1 7.071067811859902358744270457978704391088e-1
40b9213244698af7 -7.071067811864617030561845793329365007353e-1 7.071067811866333457455041344606499025184e-1
40b9213244698af8 -7.071067811858185931851073345620405032045e-1 7.071067811872764556165806382184089635689e-1
40b921fb54442d18 -2.508076644653779306958983003004324074660e-13 9.999999999999999999999999685477577227112e-1
40c3878000000000 6.360869563962335809491541738708288176338e-1 -7.716173818043344460732092488491606759713e-1
40e67e57cdd4dc53 -9.999999999999999999999735312094151048108e-1 -7.275821683479494014956423499420912511440e-12
40e67e57cdd4dc54 -9.999999999999999999999999999999907614219e-1 1.359307039318883638248065122631353755318e-16
40e67e57cdd4dc55 -9.999999999999999999999735292313630242617e-1 7.276093544887357791684073105249327233830e-12
40f0000000000000 6.920654538227232477333303343498364159601e-1 -7.218347509126643010867607642053902628301e-1
40f65a1dd290660e 9.999999999999999999998941175921936644727e-1 1.455214127242692261983345240396906818345e-11
40f65a1dd290660f 9.999999999999999999999999999999744520415e-1 2.260440600708131933410092212890194627742e-16
40f65a1dd2906610 9.999999999999999999998941241709416645254e-1 -1.455168918430678099344677043339318174730e-11
40f67e57cdd4dc53 1.455164336695898802991246183444087890629e-11 -9.999999999999999999998941248376604192432e-1
40f67e57cdd4dc54 -2.718614078637767276496130245262682394507e-16 -9.999999999999999999999999999999630456875e-1
40f67e57cdd4dc55 -1.455218708977471558336776100292103905588e-11 -9.999999999999999999998941169254520970469e-1
40f921eec34682f5 -7.071067811987685681269875246949613145924e-1 7.071067811743264806744899811899623022140e-1
40f921eec34682f6 -7.071067811884788101898605082154164937005e-1 7.071067811846162386118229411688972107097e-1
40f921eec34682f7 -7.071067811781890522525837560506482780927e-1 7.071067811949059965490061654626095424209e-1
40f921fb54442d18 -4.012922631446046891134362076496656757664e-12 9.999999999999999999999919482259770140673e-1
40fe240c9fbe76c9 -9.986640823432245253101441505994574890905e-1 5.167253271870138266287687326741910977895e-2
4100dec1da5fa53e 9.999999999999999999995764953946021859217e-1 2.910342266462190794818566710261944175933e-11
4100dec1da5fa53f 9.999999999999999999999999999999168527968e-1 -4.077921117956650914744195367893960801438e-16
4100dec1da5fa540 9.999999999999999999995764716579772193328e-1 -2.910423824884549927836861559627966623015e-11
41065a1dd290660e 2.910428254485384523966382317642481133930e-11 -9.999999999999999999995764703687746578909e-1
41065a1dd290660f 4.520881201416263866820184425780273756199e-16 -9.999999999999999999999999999998978081658e-1
41065a1dd2906610 -2.910337836861356198689045952247318810676e-11 -9.999999999999999999995764966837666581016e-1
41067e57cdd4dc53 -2.910328673391797605982184235367418887983e-11 9.999999999999999999995764993506416769729e-1
41067e57cdd4dc54 5.437228157275534552992260490525163859984e-16 9.999999999999999999999999999998521827498e-1
41067e57cdd4dc55 2.910437417954943116673244034522115481749e-11 9.999999999999999999995764677018083881877e-1
410bf9b3c6059d23 9.999999999999999999995764743249310330673e-1 2.910414661414991335129999842748296023021e-11
410bf9b3c6059d24 9.999999999999999999999999999999500222441e-1 3.161574162097380228572119303149017144447e-16
410bf9b3c6059d25 9.999999999999999999995764927277147110818e-1 -2.910351429931749387525428427141807847522e-11
410c1dedc14a1368 -9.999999999999999999995765033066626908678e-1 -2.910315080321404417145801760472839825436e-11
410c1dedc14a1369 -9.999999999999999999999999999997690355466e-1 6.796535196594418191240325613156266454016e-16
410c1dedc14a136a -9.999999999999999999995764637456210798864e-1 2.910451011025336305509626509416210563875e-11
4110dec1da5fa53e 5.820684532924381589634668333817627051893e-11 -9.999999999999999999983059815784087436867e-1
4110dec1da5fa53f -8.155842235913301829488390735787243467405e-16 -9.999999999999999999999999999996674111871e-1
4110dec1da5fa540 -5.820847649769099855671257825301659339693e-11 -9.999999999999999999983058866319088773312e-1
41139c6fd67805a6 -9.999999999999999999983059315271233846318e-1 -5.820770520947575318782483831948116570271e-11
41139c6fd67805a7 -9.999999999999999999999999999999990189318e-1 -4.429600834596129520759890578863537863714e-17
41139c6fd67805a8 -9.999999999999999999983059366838574518755e-1 5.820761661745906126523442327175030243547e-11
41165a1dd290660e -5.820856508970769047930299330074302245772e-11 9.999999999999999999983058814750986315637e-1
41165a1dd290660f -9.041762402832527733640368851559623518114e-16 9.999999999999999999999999999995912326633e-1
41165a1dd2906610 5.820675673722712397375626829044097311076e-11 9.999999999999999999983059867350666324065e-1
41193c05c9ed3cbb 9.999999999999999999983059473515151170871e-1 5.820743334806788941109718916700632846261e-11
41193c05c9ed3cbc 9.999999999999999999999999999999741069945e-1 -2.275653995178154324420141187376342310275e-16
41193c05c9ed3cbd 9.999999999999999999983059208594158955456e-1 -5.820788847886692504196207242422223954436e-11
411bf9b3c6059d23 5.820829322829982670257534414828179239977e-11 -9.999999999999999999983058972997241322694e-1
411bf9b3c6059d24 6.323148324194760457144238606297718272130e-16 -9.999999999999999999999999999998000889764e-1
411bf9b3c6059d25 -5.820702859863498775048391744292651724363e-11 -9.999999999999999999983059709108588443270e-1
411e1e9c124264a0 -7.071067811295006932821202289080987876178e-1 -7.071067812435943555149661622070632766320e-1
411e1e9c124264a1 -7.071067811706597250331628589450288273692e-1 -7.071067812024353237681688864101906436171e-1
411e1e9c124264a2 -7.071067812118187567818097180184449250765e-1 -7.071067811612762920189758396496964086417e-1
411e1e9f3681cf2a -9.999999999999999999999856704630024638658e-1 5.353417039151000842326725158668349480323e-12
411e848000000000 1.778312015182589000850909311047512619286e-1 -9.840610061203382493619429999434212989337e-1
411edb9bbd6273d0 -9.999999999999999999983059631758329409172e-1 -5.820716148666002563436954001452718919115e-11
411edb9bbd6273d1 -9.999999999999999999999999999998752864320e-1 4.994268073815921600916271432638870216463e-16
411edb9bbd6273d2 -9.999999999999999999983059050349004305906e-1 5.820816034027478881868972157668987458825e-11
41223d98d86bd572 9.999999999999999999932238262114339536438e-1 1.164145505387195690839743103511761535355e-10
41223d98d86bd573 9.999999999999999999999999999997025572445e-1 -7.712882152453688877412401677901029003164e-16
41223d98d86bd574 9.999999999999999999932236466319023554202e-1 -1.164160931151500598217497823786072514726e-10
41239c6fd67805a6 1.164154104189515063754524609624145938093e-10 -9.999999999999999999932237261084935385274e-1
41239c6fd67805a7 8.859201669192259041519781157727067035948e-17 -9.999999999999999999999999999999960757273e-1
41239c6fd67805a8 -1.164152332349181225302716317674371512961e-10 -9.999999999999999999932237467354298075021e-1
4124fb46d48435da -9.999999999999999999932236260048137293978e-1 -1.164162702991834436669306115735669572256e-10
4124fb46d48435db -9.999999999999999999999999999995502001968e-1 -9.484722486292140685716357909445787366286e-16
4124fb46d48435dc -9.999999999999999999932238468382178655707e-1 1.164143733546861852387934811561809743931e-10
41265a1dd290660e -1.164171301794153809584087621846332431485e-10 9.999999999999999999932235259003945262548e-1
41265a1dd290660f -1.808352480566505546728073770311185508195e-15 9.999999999999999999999999999983649306530e-1
41265a1dd2906610 1.164135134744542479473153305448387213994e-10 9.999999999999999999932239469402665296261e-1
4127dd2ecbe10c87 9.999999999999999999932238895085333469638e-1 1.164140068159038415305190148094701851188e-10
4127dd2ecbe10c88 9.999999999999999999999999999991353729942e-1 -1.315011030972922343040466216842343550199e-15
4127dd2ecbe10c89 9.999999999999999999932235833336685935996e-1 -1.164166368379657873752050779201811620153e-10
41293c05c9ed3cbb 1.164148666961357788219971654207630535181e-10 -9.999999999999999999932237894060604683483e-1
41293c05c9ed3cbc -4.551307990356308648840282374752566773506e-16 -9.999999999999999999999999999998964279779e-1
41293c05c9ed3cbd -1.164157769577338500837269273090654905377e-10 -9.999999999999999999932236834376635821824e-1
412a9adcc7f96cef -9.999999999999999999932236893028481957196e-1 -1.164157265763677161134753160319698454619e-10
412a9adcc7f96cf0 -9.999999999999999999999999999999180889483e-1 -4.047494329016606132724097418921667164861e-16
412a9adcc7f96cf1 -9.999999999999999999932237835409191767519e-1 1.164149170775019127922487766978637419316e-10
412bf9b3c6059d23 -1.164165864565996534049534666430905603144e-10 9.999999999999999999932235891988965290775e-1
412bf9b3c6059d24 -1.264629664838952091428847721259290841015e-15 9.999999999999999999999999999992003559054e-1
412bf9b3c6059d25 1.164140571972699755007706260865759168328e-10 9.999999999999999999932238836434353773082e-1
412d588ac411cd57 9.999999999999999999932234890942054684223e-1 1.164174463368315906964316172541251974398e-10
412d588ac411cd58 9.999999999999999999999999999977432288492e-1 2.124509896776243569585285700625479905941e-15
412d588ac411cd59 9.999999999999999999932239837452121838512e-1 -1.164131973170380382092924754752020158770e-10
412d7cc4bf56439c 9.999999999999999999932239528053371057834e-1 1.164134630930881139770637192677298007053e-10
412d7cc4bf56439d 9.999999999999999999999999999982725542436e-1 -1.858733846700475798339692265894195437453e-15
412d7cc4bf56439e 9.999999999999999999932235200351391972786e-1 -1.164171805607815149286603734617206557837e-10
412e847c00b386ba -7.071067811102884946037858567337620353099e-1 -7.071067812628065541896785938871536101519e-1
412e847c00b386bb -7.071067811926065581057119401359580179537e-1 -7.071067811804884906959248656217978628708e-1
412e847c00b386bc -7.071067812749246215980549396838007973698e-1 -7.071067810981704271925880535022531427995e-1
412e847d92d33bff -9.999999999999999999997620150393218445078e-1 -2.181673489219482137644062820799857533717e-11
412e847e00000000 -9.773520315382229548395369565325506914900e-1 2.116199575846012748270049814374172700433e-1
412e848000000000 -3.499935021712929521176524867807714690614e-1 9.367521275331447869385325350749187757081e-1
b9b4484bfeebc2a0 -1.000000000000000083336420607585985350931e-30 1.000000000000000000000000000000000000000e+0
bfe0000000000000 -4.794255386042030002732879352155713880818e-1 8.775825618903727161162815826038296519916e-1
bff921fb54442d18 -9.999999999999999999999999999999981253003e-1 6.123233995736765886130329661375001464640e-17
c00921f9f01b866e -2.653589793352730077419785079371113475691e-6 -9.999999999964792306043009097337437036147e-1
c014000000000000 9.589242746631384688931544061559939733525e-1 2.836621854632262644666391715135573083344e-1
c01c463abeccb2ba -7.071067811865467015237482356083588264591e-1 7.071067811865483472779404886003816503263e-1
c022d97c7f3321d2 -3.673940397442059531678197796824920524208e-16 -9.999999999999999999999999999999325108098e-1
c0278fdb9effea47 7.071067811865472210973819769047396573657e-1 7.071067811865478277043067473048283234695e-1
c036000000000000 8.851309290403875921690256815772332463289e-3 -9.999608263946371264541747392126937741360e-1
c063896a5e80ff0d 7.071067811865650213088770771779333698784e-1 7.071067811865300274928116465988148362667e-1
c098880b30e00b84 7.071067811865587706685975817807948606435e-1 7.071067811865362781330911422500355536042e-1
c0c3878000000000 -6.360869563962335809491541738708288176338e-1 -7.716173818043344460732092488491606759713e-1
c0f65a1dd2906610 -9.999999999999999999998941241709416645254e-1 -1.455168918430678099344677043339318174730e-11
c0f921fb54442d18 4.012922631446046891134362076496656757664e-12 9.999999999999999999999919482259770140673e-1
c1065a1dd2906610 2.910337836861356198689045952247318810676e-11 -9.999999999999999999995764966837666581016e-1
c10c1dedc14a1368 9.999999999999999999995765033066626908678e-1 -2.910315080321404417145801760472839825436e-11
c1139c6fd67805a7 9.999999999999999999999999999999990189318e-1 -4.429600834596129520759890578863537863714e-17
c1193c05c9ed3cbd -9.999999999999999999983059208594158955456e-1 -5.820788847886692504196207242422223954436e-11
c11e1e9f3681cf2a 9.999999999999999999999856704630024638658e-1 5.353417039151000842326725158668349480323e-12
c1223d98d86bd574 -9.999999999999999999932236466319023554202e-1 -1.164160931151500598217497823786072514726e-10
c1265a1dd290660e 1.164171301794153809584087621846332431485e-10 9.999999999999999999932235259003945262548e-1
c1293c05c9ed3cbc 4.551307990356308648840282374752566773506e-16 -9.999999999999999999999999999998964279779e-1
c12bf9b3c6059d25 -1.164140571972699755007706260865759168328e-10 9.999999999999999999932238836434353773082e-1
c12e847c00b386ba 7.071067811102884946037858567337620353099e-1 -7.071067812628065541896785938871536101519e-1
";

    /// [`GOLDEN`]'s rows, parsed.
    fn golden_rows() -> impl Iterator<Item = (u64, &'static str, &'static str)> {
        GOLDEN.lines().map(|row| {
            let mut fields = row.split(' ');
            let mut field = || fields.next().expect("a golden row has three fields");
            let bits = u64::from_str_radix(field(), 16).expect("an argument's bits in hex");
            (bits, field(), field())
        })
    }
}
