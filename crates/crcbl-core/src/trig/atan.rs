//! The arc tangent in `f64`, computed without calling `atan` or `atan2`.
//!
//! The same rule as [`sin`](super::sin) and [`cos`](super::cos): the
//! platform's `atan2` is the C runtime's on `x86_64-pc-windows-msvc` and
//! Apple's or glibc's elsewhere, each rounding its last bit in its own way, so
//! a simulation that turns a rotation into an angle with it reaches different
//! bits on different targets.
//!
//! # The construction
//!
//! A transcription of musl's `atan2` as the Rust `libm` crate (0.2.16) carries
//! it, over `fdlibm`'s `s_atan.c` for [`atan`] with musl's tiny-argument cut at
//! `2^-27`. Every constant is the hexadecimal value `fdlibm` names as the
//! intended one, written as its bits.
//!
//! musl's [`atan2`] differs from Sun's and FreeBSD's `e_atan2.c` in one band:
//! it returns `±π/2` outright only once `|y / x|` passes `2^64`, where
//! `e_atan2.c` does past `2^60`. For `x < 0` and `|y / x|` between the two,
//! `π - (z - π_lo)` lands one ulp above `π/2`, where `e_atan2.c` and glibc
//! return `π/2` itself. `musl_s_wide_band_is_one_ulp_above_half_pi` records it.
//!
//! 1. **[`atan`].** The argument's magnitude picks one of five intervals; on
//!    four of them `atan(t) = atan(c) + atan((t - c) / (1 + c·t))` moves it
//!    near zero for `c` in `{1/2, 1, 3/2, ∞}`, with `atan(c)` held in two
//!    parts ([`ATAN_HI`], [`ATAN_LO`]). An odd polynomial of degree 23
//!    ([`ATAN_POLY`], split into odd and even halves over `t⁴`) does the rest.
//! 2. **[`atan2`].** The quadrant from the two signs, IEEE-754's special cases
//!    for zeros and infinities, and `atan(|y / x|)` folded into the quadrant
//!    against `π` held in two parts.
//!
//! `fdlibm` documents [`atan`] under one unit in the last place, and [`atan2`]
//! adds the rounding of `y / x`. The tests hold [`atan2`] within
//! [`MAX_ATAN2_ULP`] of `bc`'s value at every row of a golden table, and within
//! one more than that of the platform's own `atan2` over a sweep.
//!
//! # What is claimed
//!
//! **Bit-identical on every target.** Every step is `+`, `-`, `*`, `/`, `abs`
//! or a bit test, each of which IEEE-754 specifies exactly, and Rust contracts
//! no multiply-add into an FMA on its own. A digest over a fixed set of
//! arguments is pinned by `the_outputs_hash_to_the_pinned_digest`.
//!
//! **Not correctly rounded**, and no faster than the platform's.

/// `atan(1/2)`, `atan(1)`, `atan(3/2)` and `atan(∞)`, rounded: `fdlibm`'s
/// `atanhi`.
const ATAN_HI: [f64; 4] = [
    f64::from_bits(0x3fdd_ac67_0561_bb4f),
    f64::from_bits(0x3fe9_21fb_5444_2d18),
    f64::from_bits(0x3fef_730b_d281_f69b),
    f64::from_bits(0x3ff9_21fb_5444_2d18),
];

/// What each of [`ATAN_HI`] leaves of the true value, rounded: `fdlibm`'s
/// `atanlo`.
const ATAN_LO: [f64; 4] = [
    f64::from_bits(0x3c7a_2b7f_222f_65e2),
    f64::from_bits(0x3c81_a626_3314_5c07),
    f64::from_bits(0x3c70_0788_7af0_cbbd),
    f64::from_bits(0x3c91_a626_3314_5c07),
];

/// The reduced argument's polynomial, `atan(t) ≈ t - t·Σ ATAN_POLY[i]·t^(2i+2)`
/// with alternating signs folded in: `fdlibm`'s `aT`.
const ATAN_POLY: [f64; 11] = [
    f64::from_bits(0x3fd5_5555_5555_550d),
    f64::from_bits(0xbfc9_9999_9998_ebc4),
    f64::from_bits(0x3fc2_4924_9200_83ff),
    f64::from_bits(0xbfbc_71c6_fe23_1671),
    f64::from_bits(0x3fb7_45cd_c54c_206e),
    f64::from_bits(0xbfb3_b0f2_af74_9a6d),
    f64::from_bits(0x3fb1_0d66_a0d0_3d51),
    f64::from_bits(0xbfad_de2d_52de_fd9a),
    f64::from_bits(0x3fa9_7b4b_2476_0deb),
    f64::from_bits(0xbfa2_b444_2c6a_6c2f),
    f64::from_bits(0x3f90_ad3a_e322_da11),
];

/// `π`, rounded: `fdlibm`'s `pi`.
const PI: f64 = f64::from_bits(0x4009_21fb_5444_2d18);

/// What [`PI`] leaves of `π`, rounded: `fdlibm`'s `pi_lo`.
const PI_LO: f64 = f64::from_bits(0x3ca1_a626_3314_5c07);

/// How far [`atan2`] may land from the true value rounded to an `f64`, in
/// units of the last place: the measured ceiling over the golden table
/// `atan2_matches_the_golden_table` checks, not a derived bound.
pub const MAX_ATAN2_ULP: u64 = 1;

/// The upper 32 bits of `x`, as `fdlibm`'s `GET_HIGH_WORD` reads them.
fn high_word(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

/// The lower 32 bits of `x`: `fdlibm`'s `GET_LOW_WORD`.
fn low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

/// The arc tangent of `x`, in `[-π/2, π/2]`, from IEEE-754 arithmetic alone.
///
/// A NaN is returned as itself. Bit-identical on every target; see the module
/// docs.
#[must_use]
pub fn atan(x: f64) -> f64 {
    let negative = high_word(x) >> 31 != 0;
    let ix = high_word(x) & 0x7fff_ffff;
    // |x| >= 2^66: the answer is ±π/2 to the last bit.
    if ix >= 0x4410_0000 {
        if x.is_nan() {
            return x;
        }
        let z = ATAN_HI[3] + ATAN_LO[3];
        return if negative { -z } else { z };
    }
    let (id, t) = if ix < 0x3fdc_0000 {
        // |x| < 7/16, and under 2^-27 atan(x) rounds to x.
        if ix < 0x3e40_0000 {
            return x;
        }
        (None, x)
    } else {
        let a = x.abs();
        if ix < 0x3fe6_0000 {
            // 7/16 <= |x| < 11/16
            (Some(0), (2.0 * a - 1.0) / (2.0 + a))
        } else if ix < 0x3ff3_0000 {
            // 11/16 <= |x| < 19/16
            (Some(1), (a - 1.0) / (a + 1.0))
        } else if ix < 0x4003_8000 {
            // 19/16 <= |x| < 39/16
            (Some(2), (a - 1.5) / (1.0 + 1.5 * a))
        } else {
            // 39/16 <= |x| < 2^66
            (Some(3), -1.0 / a)
        }
    };
    let z = t * t;
    let w = z * z;
    let p = &ATAN_POLY;
    let s1 = z * (p[0] + w * (p[2] + w * (p[4] + w * (p[6] + w * (p[8] + w * p[10])))));
    let s2 = w * (p[1] + w * (p[3] + w * (p[5] + w * (p[7] + w * p[9]))));
    match id {
        None => t - t * (s1 + s2),
        Some(id) => {
            let z = ATAN_HI[id] - ((t * (s1 + s2) - ATAN_LO[id]) - t);
            if negative { -z } else { z }
        }
    }
}

/// The angle of the point `(x, y)` from the positive `x` axis, in `[-π, π]`,
/// from IEEE-754 arithmetic alone.
///
/// Its special cases are IEEE-754's and C's: a NaN in either argument is a
/// NaN; `atan2(±0, x)` is `±0` for `x` at or above `+0` and `±π` for `x` at or
/// below `-0`; `atan2(y, ±0)` is `±π/2` for any other `y`; and an infinite
/// argument gives the multiple of `π/4` its signs point at. Bit-identical on
/// every target; see the module docs.
#[must_use]
pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    let (hx, lx) = (high_word(x), low_word(x));
    let (hy, ly) = (high_word(y), low_word(y));
    if hx == 0x3ff0_0000 && lx == 0 {
        // x == 1.0
        return atan(y);
    }
    // Bit 0 is y's sign and bit 1 is x's.
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    let ix = hx & 0x7fff_ffff;
    let iy = hy & 0x7fff_ffff;

    if iy | ly == 0 {
        return match m {
            0 | 1 => y,
            2 => PI,
            _ => -PI,
        };
    }
    if ix | lx == 0 {
        return if m & 1 != 0 { -PI / 2.0 } else { PI / 2.0 };
    }
    if ix == 0x7ff0_0000 {
        return if iy == 0x7ff0_0000 {
            match m {
                0 => PI / 4.0,
                1 => -PI / 4.0,
                2 => 3.0 * PI / 4.0,
                _ => -3.0 * PI / 4.0,
            }
        } else {
            match m {
                0 => 0.0,
                1 => -0.0,
                2 => PI,
                _ => -PI,
            }
        };
    }
    // |y / x| > 2^64, or y infinite: ±π/2.
    if ix.wrapping_add(64 << 20) < iy || iy == 0x7ff0_0000 {
        return if m & 1 != 0 { -PI / 2.0 } else { PI / 2.0 };
    }
    // |y / x| < 2^-64 with x negative is π to the last bit, and taking the
    // quotient would underflow.
    let z = if m & 2 != 0 && iy.wrapping_add(64 << 20) < ix {
        0.0
    } else {
        atan((y / x).abs())
    };
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI as CORE_PI};

    /// The distance between two finite `f64`s of the same sign in units of
    /// the last place, as the count of `f64`s between them.
    fn ulps_apart(a: f64, b: f64) -> u64 {
        if a == b {
            return 0;
        }
        assert_eq!(
            a.is_sign_negative(),
            b.is_sign_negative(),
            "{a:e} and {b:e} have different signs"
        );
        a.to_bits().abs_diff(b.to_bits())
    }

    /// One row per point: `y`'s bits, `x`'s bits, then `atan2(y, x)` from
    /// `bc -l` over the arguments' exact decimal expansions (Python's
    /// `Decimal(float)`), cut to forty significant digits. The `bc` program
    /// is `scale=140`, then `pi=4*a(1)`, then one line per point that folds the
    /// quadrant by hand, since `bc` has only the one-argument `a`:
    ///
    /// ```text
    /// a((y)/(x))       for x > 0
    /// a((y)/(x))+pi    for x < 0 and y >= 0
    /// a((y)/(x))-pi    for x < 0 and y < 0
    /// ```
    ///
    /// The points cover all four quadrants; ratios on both sides of each of
    /// [`atan`]'s interval edges (7/16, 11/16, 19/16, 39/16) and of its
    /// `2^-27` and `2^66` cuts; `x == 1`, which takes [`atan`] directly; and
    /// points a hair either side of each axis, where `π - (z - π_lo)` cancels.
    const GOLDEN: &str = "\
3fdbffffffffffff 3ff0000000000000 4.124104415973872603068250103162941268745e-1
bfdbffffffffffff 3ff0000000000000 -4.124104415973872603068250103162941268745e-1
3fdbffffffffffff bff0000000000000 2.729182211992405978155818372963208757323e+0
bfdbffffffffffff bff0000000000000 -2.729182211992405978155818372963208757323e+0
3ff9e66666666666 400d99999999999a 4.124104415973872691217105226259131044191e-1
bff9e66666666666 400d99999999999a -4.124104415973872691217105226259131044191e-1
3ff9e66666666666 c00d99999999999a 2.729182211992405969340932860653589779778e+0
bff9e66666666666 c00d99999999999a -2.729182211992405969340932860653589779778e+0
3fdc000000000000 3ff0000000000000 4.124104415973873068997912896671269370468e-1
bfdc000000000000 3ff0000000000000 -4.124104415973873068997912896671269370468e-1
3fdc000000000000 bff0000000000000 2.729182211992405931562852093612375947150e+0
bfdc000000000000 bff0000000000000 -2.729182211992405931562852093612375947150e+0
3ff9e66666666667 400d99999999999a 4.124104415973873194924848786808646037482e-1
bff9e66666666667 400d99999999999a -4.124104415973873194924848786808646037482e-1
3ff9e66666666667 c00d99999999999a 2.729182211992405918970158504598638280449e+0
bff9e66666666667 c00d99999999999a -2.729182211992405918970158504598638280449e+0
3fdc000000000001 3ff0000000000000 4.124104415973873534927575690179578476777e-1
bfdc000000000001 3ff0000000000000 -4.124104415973873534927575690179578476777e-1
3fdc000000000001 bff0000000000000 2.729182211992405884969885814261545036519e+0
bfdc000000000001 bff0000000000000 -2.729182211992405884969885814261545036519e+0
3ff9e66666666668 400d99999999999a 4.124104415973873698632592347358138830142e-1
bff9e66666666668 400d99999999999a -4.124104415973873698632592347358138830142e-1
3ff9e66666666668 c00d99999999999a 2.729182211992405868599384148543689001183e+0
bff9e66666666668 c00d99999999999a -2.729182211992405868599384148543689001183e+0
3fe5ffffffffffff 3ff0000000000000 6.022873461349641062929730114349785122718e-1
bfe5ffffffffffff 3ff0000000000000 -6.022873461349641062929730114349785122718e-1
3fe5ffffffffffff bff0000000000000 2.539305307454829132169670371844524371925e+0
bfe5ffffffffffff bff0000000000000 -2.539305307454829132169670371844524371925e+0
4004599999999999 400d99999999999a 6.022873461349641103680621834765634546258e-1
c004599999999999 400d99999999999a -6.022873461349641103680621834765634546258e-1
4004599999999999 c00d99999999999a 2.539305307454829128094581199802939429571e+0
c004599999999999 c00d99999999999a -2.539305307454829128094581199802939429571e+0
3fe6000000000000 3ff0000000000000 6.022873461349641816821226942042329099995e-1
bfe6000000000000 3ff0000000000000 -6.022873461349641816821226942042329099995e-1
3fe6000000000000 bff0000000000000 2.539305307454829056780520689075269974198e+0
bfe6000000000000 bff0000000000000 -2.539305307454829056780520689075269974198e+0
400459999999999a 400d99999999999a 6.022873461349641918698456243081851184952e-1
c00459999999999a 400d99999999999a -6.022873461349641918698456243081851184952e-1
400459999999999a c00d99999999999a 2.539305307454829046592797758971317765702e+0
c00459999999999a c00d99999999999a -2.539305307454829046592797758971317765702e+0
3fe6000000000001 3ff0000000000000 6.022873461349642570712723769734794928818e-1
bfe6000000000001 3ff0000000000000 -6.022873461349642570712723769734794928818e-1
3fe6000000000001 bff0000000000000 2.539305307454828981391371006306023391315e+0
bfe6000000000001 bff0000000000000 -2.539305307454828981391371006306023391315e+0
400459999999999b 400d99999999999a 6.022873461349642733716290651397976488712e-1
c00459999999999b 400d99999999999a -6.022873461349642733716290651397976488712e-1
400459999999999b c00d99999999999a 2.539305307454828965091014318139705235326e+0
c00459999999999b c00d99999999999a -2.539305307454828965091014318139705235326e+0
3ff2ffffffffffff 3ff0000000000000 8.709034570756528610114553479170667868628e-1
bff2ffffffffffff 3ff0000000000000 -8.709034570756528610114553479170667868628e-1
3ff2ffffffffffff bff0000000000000 2.270689196514140377451188035362436097334e+0
bff2ffffffffffff bff0000000000000 -2.270689196514140377451188035362436097334e+0
4011933333333333 400d99999999999a 8.709034570756529095657795752488165937036e-1
c011933333333333 400d99999999999a -8.709034570756529095657795752488165937036e-1
4011933333333333 c00d99999999999a 2.270689196514140328896863808030686290494e+0
c011933333333333 c00d99999999999a -2.270689196514140328896863808030686290494e+0
3ff3000000000000 3ff0000000000000 8.709034570756529531401731125978140940299e-1
bff3000000000000 3ff0000000000000 -8.709034570756529531401731125978140940299e-1
3ff3000000000000 bff0000000000000 2.270689196514140285322470270681688790167e+0
bff3000000000000 bff0000000000000 -2.270689196514140285322470270681688790167e+0
3ff3000000000001 3ff0000000000000 8.709034570756530452688908772785412429080e-1
bff3000000000001 3ff0000000000000 -8.709034570756530452688908772785412429080e-1
3ff3000000000001 bff0000000000000 2.270689196514140193193752506000961641289e+0
bff3000000000001 bff0000000000000 -2.270689196514140193193752506000961641289e+0
4011933333333334 400d99999999999a 8.709034570756530091643933749036613968485e-1
c011933333333334 400d99999999999a -8.709034570756530091643933749036613968485e-1
4011933333333334 c00d99999999999a 2.270689196514140229298250008375841487349e+0
c011933333333334 c00d99999999999a -2.270689196514140229298250008375841487349e+0
40037fffffffffff 3ff0000000000000 1.181479604961755698325201955511989930710e+0
c0037fffffffffff 3ff0000000000000 -1.181479604961755698325201955511989930710e+0
40037fffffffffff bff0000000000000 1.960113048628037540137441427767512953487e+0
c0037fffffffffff bff0000000000000 -1.960113048628037540137441427767512953487e+0
4022099999999999 400d99999999999a 1.181479604961755703944788908853521985012e+0
c022099999999999 400d99999999999a -1.181479604961755703944788908853521985012e+0
4022099999999999 c00d99999999999a 1.960113048628037534517854474425980899185e+0
c022099999999999 c00d99999999999a -1.960113048628037534517854474425980899185e+0
4003800000000000 3ff0000000000000 1.181479604961755762302038039707852321316e+0
c003800000000000 3ff0000000000000 -1.181479604961755762302038039707852321316e+0
4003800000000000 bff0000000000000 1.960113048628037476160605343571650562881e+0
c003800000000000 bff0000000000000 -1.960113048628037476160605343571650562881e+0
402209999999999a 400d99999999999a 1.181479604961755773108936026903096857982e+0
c02209999999999a 400d99999999999a -1.181479604961755773108936026903096857982e+0
402209999999999a c00d99999999999a 1.960113048628037465353707356376406026215e+0
c02209999999999a c00d99999999999a -1.960113048628037465353707356376406026215e+0
4003800000000001 3ff0000000000000 1.181479604961755826278874123903694758374e+0
c003800000000001 3ff0000000000000 -1.181479604961755826278874123903694758374e+0
4003800000000001 bff0000000000000 1.960113048628037412183769259375808125823e+0
c003800000000001 bff0000000000000 -1.960113048628037412183769259375808125823e+0
402209999999999b 400d99999999999a 1.181479604961755842273083144952648410515e+0
c02209999999999b 400d99999999999a -1.181479604961755842273083144952648410515e+0
402209999999999b c00d99999999999a 1.960113048628037396189560238326854473682e+0
c02209999999999b c00d99999999999a -1.960113048628037396189560238326854473682e+0
3e3ffffde7210be9 3ff0000000000000 7.450573146343230849062457068253498338555e-9
be3ffffde7210be9 3ff0000000000000 -7.450573146343230849062457068253498338555e-9
3e3ffffde7210be9 bff0000000000000 3.141592646139220092119412534217045815944e+0
be3ffffde7210be9 bff0000000000000 -3.141592646139220092119412534217045815944e+0
3e5d9997a8fe916b 400d99999999999a 7.450573146343230737281650909460614090283e-9
be5d9997a8fe916b 400d99999999999a -7.450573146343230737281650909460614090283e-9
3e5d9997a8fe916b c00d99999999999a 3.141592646139220092119412645997851974737e+0
be5d9997a8fe916b c00d99999999999a -3.141592646139220092119412645997851974737e+0
3e4000010c6f7a0b 3ff0000000000000 7.450588047504424298030059526882476321689e-9
be4000010c6f7a0b 3ff0000000000000 -7.450588047504424298030059526882476321689e-9
3e4000010c6f7a0b bff0000000000000 3.141592646139205190958219085249443357315e+0
be4000010c6f7a0b bff0000000000000 -3.141592646139205190958219085249443357315e+0
3e5d999b8a34a1c8 400d99999999999a 7.450588047504424342742167370565055208995e-9
be5d999b8a34a1c8 400d99999999999a -7.450588047504424342742167370565055208995e-9
3e5d999b8a34a1c8 c00d99999999999a 3.141592646139205190958219040537335513632e+0
be5d999b8a34a1c8 c00d99999999999a -3.141592646139205190958219040537335513632e+0
441000001ad7f29b 3ff0000000000000 1.570796326794896619217769165838935217546e+0
c41000001ad7f29b 3ff0000000000000 -1.570796326794896619217769165838935217546e+0
441000001ad7f29b bff0000000000000 1.570796326794896619244874217440567666651e+0
c41000001ad7f29b bff0000000000000 -1.570796326794896619244874217440567666651e+0
442d9999cb42b406 400d99999999999a 1.570796326794896619217769165838935218278e+0
c42d9999cb42b406 400d99999999999a -1.570796326794896619217769165838935218278e+0
442d9999cb42b406 c00d99999999999a 1.570796326794896619244874217440567665919e+0
c42d9999cb42b406 c00d99999999999a -1.570796326794896619244874217440567665919e+0
3fb999999999999a 3ff0000000000000 9.966865249116203287459970713127078733950e-2
bfb999999999999a 3ff0000000000000 -9.966865249116203287459970713127078733950e-2
3fb999999999999a bff0000000000000 3.041924001098631205588043676148232096858e+0
bfb999999999999a bff0000000000000 -3.041924001098631205588043676148232096858e+0
3fd7ae147ae147af 400d99999999999a 9.966865249116203629112761272112858702716e-2
bfd7ae147ae147af 400d99999999999a -9.966865249116203629112761272112858702716e-2
3fd7ae147ae147af c00d99999999999a 3.041924001098631202171515770558374297170e+0
bfd7ae147ae147af c00d99999999999a -3.041924001098631202171515770558374297170e+0
3fd3333333333333 3ff0000000000000 2.914567944778670818100722854222806997775e-1
bfd3333333333333 3ff0000000000000 -2.914567944778670818100722854222806997775e-1
3fd3333333333333 bff0000000000000 2.850135859111926156652571097857222184420e+0
bfd3333333333333 bff0000000000000 -2.850135859111926156652571097857222184420e+0
3ff1c28f5c28f5c3 400d99999999999a 2.914567944778671030069909306335505880451e-1
bff1c28f5c28f5c3 400d99999999999a -2.914567944778671030069909306335505880451e-1
3ff1c28f5c28f5c3 c00d99999999999a 2.850135859111926135455652452645952296152e+0
bff1c28f5c28f5c3 c00d99999999999a -2.850135859111926135455652452645952296152e+0
3fe0000000000000 3ff0000000000000 4.636476090008061162142562314612144020285e-1
bfe0000000000000 3ff0000000000000 -4.636476090008061162142562314612144020285e-1
3fe0000000000000 bff0000000000000 2.677945044588987122248387151818288482169e+0
bfe0000000000000 bff0000000000000 -2.677945044588987122248387151818288482169e+0
3ffd99999999999a 400d99999999999a 4.636476090008061162142562314612144020285e-1
bffd99999999999a 400d99999999999a -4.636476090008061162142562314612144020285e-1
3ffd99999999999a c00d99999999999a 2.677945044588987122248387151818288482169e+0
bffd99999999999a c00d99999999999a -2.677945044588987122248387151818288482169e+0
3ff0000000000000 3ff0000000000000 7.853981633974483096156608458198757210493e-1
bff0000000000000 3ff0000000000000 -7.853981633974483096156608458198757210493e-1
3ff0000000000000 bff0000000000000 2.356194490192344928846982537459627163148e+0
bff0000000000000 bff0000000000000 -2.356194490192344928846982537459627163148e+0
400d99999999999a 400d99999999999a 7.853981633974483096156608458198757210493e-1
c00d99999999999a 400d99999999999a -7.853981633974483096156608458198757210493e-1
400d99999999999a c00d99999999999a 2.356194490192344928846982537459627163148e+0
c00d99999999999a c00d99999999999a -2.356194490192344928846982537459627163148e+0
3ff8000000000000 3ff0000000000000 9.827937232473290679857106110146660144969e-1
bff8000000000000 3ff0000000000000 -9.827937232473290679857106110146660144969e-1
3ff8000000000000 bff0000000000000 2.158798930342464170476932772264836869700e+0
bff8000000000000 bff0000000000000 -2.158798930342464170476932772264836869700e+0
4016333333333334 400d99999999999a 9.827937232473291049162062326414909831651e-1
c016333333333334 400d99999999999a -9.827937232473291049162062326414909831651e-1
4016333333333334 c00d99999999999a 2.158798930342464133546437150638011901032e+0
c016333333333334 c00d99999999999a -2.158798930342464133546437150638011901032e+0
4000000000000000 3ff0000000000000 1.107148717794090503017065460178537040070e+0
c000000000000000 3ff0000000000000 -1.107148717794090503017065460178537040070e+0
4000000000000000 bff0000000000000 2.034443935795702735445577923100965844127e+0
c000000000000000 bff0000000000000 -2.034443935795702735445577923100965844127e+0
401d99999999999a 400d99999999999a 1.107148717794090503017065460178537040070e+0
c01d99999999999a 400d99999999999a -1.107148717794090503017065460178537040070e+0
401d99999999999a c00d99999999999a 2.034443935795702735445577923100965844127e+0
c01d99999999999a c00d99999999999a -2.034443935795702735445577923100965844127e+0
4008000000000000 3ff0000000000000 1.249045772398254425829917077281090123078e+0
c008000000000000 3ff0000000000000 -1.249045772398254425829917077281090123078e+0
4008000000000000 bff0000000000000 1.892546881191538812632726305998412761119e+0
c008000000000000 bff0000000000000 -1.892546881191538812632726305998412761119e+0
4026333333333334 400d99999999999a 1.249045772398254449834739231338525953783e+0
c026333333333334 400d99999999999a -1.249045772398254449834739231338525953783e+0
4026333333333334 c00d99999999999a 1.892546881191538788627904151940976930414e+0
c026333333333334 c00d99999999999a -1.892546881191538788627904151940976930414e+0
4024000000000000 3ff0000000000000 1.471127674303734591852875571761730851855e+0
c024000000000000 3ff0000000000000 -1.471127674303734591852875571761730851855e+0
4024000000000000 bff0000000000000 1.670464979286058646609767811517772032342e+0
c024000000000000 bff0000000000000 -1.670464979286058646609767811517772032342e+0
4042800000000000 400d99999999999a 1.471127674303734587099445442245406356716e+0
c042800000000000 400d99999999999a -1.471127674303734587099445442245406356716e+0
4042800000000000 c00d99999999999a 1.670464979286058651363197941034096527481e+0
c042800000000000 c00d99999999999a -1.670464979286058651363197941034096527481e+0
40f86a0000000000 3ff0000000000000 1.570786326794896952564655004973084776860e+0
c0f86a0000000000 3ff0000000000000 -1.570786326794896952564655004973084776860e+0
40f86a0000000000 bff0000000000000 1.570806326794896285897988378306418107337e+0
c0f86a0000000000 bff0000000000000 -1.570806326794896285897988378306418107337e+0
4116954000000000 400d99999999999a 1.570786326794896952564174908530051637731e+0
c116954000000000 400d99999999999a -1.570786326794896952564174908530051637731e+0
4116954000000000 c00d99999999999a 1.570806326794896285898468474749451246467e+0
c116954000000000 c00d99999999999a -1.570806326794896285898468474749451246467e+0
4202a05f20000000 3ff0000000000000 1.570796326694896619231321691640084775432e+0
c202a05f20000000 3ff0000000000000 -1.570796326694896619231321691640084775432e+0
4202a05f20000000 bff0000000000000 1.570796326894896619231321691639418108765e+0
c202a05f20000000 bff0000000000000 -1.570796326894896619231321691639418108765e+0
42213abe64000000 400d99999999999a 1.570796326694896619231321686839120344620e+0
c2213abe64000000 400d99999999999a -1.570796326694896619231321686839120344620e+0
42213abe64000000 c00d99999999999a 1.570796326894896619231321696440382539577e+0
c2213abe64000000 c00d99999999999a -1.570796326894896619231321696440382539577e+0
3ee4f8b588e368f1 3ff0000000000000 9.999999999666667484717205725175279648010e-6
bee4f8b588e368f1 3ff0000000000000 -9.999999999666667484717205725175279648010e-6
3ee4f8b588e368f1 bff0000000000000 3.141582653589793571795158666073777708918e+0
bee4f8b588e368f1 bff0000000000000 -3.141582653589793571795158666073777708918e+0
3f03660e51d25aac 400d99999999999a 9.999999999666667508261974522607324870617e-6
bf03660e51d25aac 400d99999999999a -9.999999999666667508261974522607324870617e-6
3f03660e51d25aac c00d99999999999a 3.141582653589793571795135121304980276872e+0
bf03660e51d25aac c00d99999999999a -3.141582653589793571795135121304980276872e+0
3ddb7cdfd9d7bdbb 3ff0000000000000 1.000000000000000036428863982164408245468e-10
bddb7cdfd9d7bdbb 3ff0000000000000 -1.000000000000000036428863982164408245468e-10
3ddb7cdfd9d7bdbb bff0000000000000 3.141592653489793238462643379636616485981e+0
bddb7cdfd9d7bdbb bff0000000000000 -3.141592653489793238462643379636616485981e+0
3df96d1bdcb45c4d 400d99999999999a 9.999999999999999919123810446281953778434e-11
bdf96d1bdcb45c4d 400d99999999999a -9.999999999999999919123810446281953778434e-11
3df96d1bdcb45c4d c00d99999999999a 3.141592653489793238462643384088264779734e+0
bdf96d1bdcb45c4d c00d99999999999a -3.141592653489793238462643384088264779734e+0
3bc79ca10c924223 bff0000000000000 3.141592653589793238452643383279502884746e+0
bbc79ca10c924223 bff0000000000000 -3.141592653589793238452643383279502884746e+0
3ff0000000000000 3bc79ca10c924223 1.570796326794896619221321691639751442647e+0
3ff0000000000000 bbc79ca10c924223 1.570796326794896619241321691639751441550e+0
bff0000000000000 3bc79ca10c924223 -1.570796326794896619221321691639751442647e+0
bff0000000000000 bbc79ca10c924223 -1.570796326794896619241321691639751441550e+0
3d719799812dea11 bff0000000000000 3.141592653588793238462643383299616236901e+0
bd719799812dea11 bff0000000000000 -3.141592653588793238462643383299616236901e+0
3ff0000000000000 3d719799812dea11 1.570796326793896619231321691659864794803e+0
3ff0000000000000 bd719799812dea11 1.570796326795896619231321691619638089395e+0
bff0000000000000 3d719799812dea11 -1.570796326793896619231321691659864794803e+0
bff0000000000000 bd719799812dea11 -1.570796326795896619231321691619638089395e+0
";

    /// [`GOLDEN`]'s rows, parsed.
    fn golden_rows() -> impl Iterator<Item = (f64, f64, f64)> {
        GOLDEN.lines().map(|row| {
            let mut fields = row.split(' ');
            let mut field = || fields.next().expect("a golden row has three fields");
            let bits = |text: &str| {
                f64::from_bits(u64::from_str_radix(text, 16).expect("an argument's bits in hex"))
            };
            let y = bits(field());
            let x = bits(field());
            let value: f64 = field().parse().expect("a golden value is a decimal");
            (y, x, value)
        })
    }

    /// `fdlibm` gives each constant twice, as a decimal and as the hex words it
    /// intends; the bits above are the hex, and these are the decimals, so a
    /// slip in either transcription disagrees with the other.
    #[test]
    fn the_constants_are_fdlibm_s_decimals() {
        let hi = [
            "4.63647609000806093515e-01",
            "7.85398163397448278999e-01",
            "9.82793723247329054082e-01",
            "1.57079632679489655800e+00",
        ];
        let lo = [
            "2.26987774529616870924e-17",
            "3.06161699786838301793e-17",
            "1.39033110312309984516e-17",
            "6.12323399573676603587e-17",
        ];
        let poly = [
            "3.33333333333329318027e-01",
            "-1.99999999998764832476e-01",
            "1.42857142725034663711e-01",
            "-1.11111104054623557880e-01",
            "9.09088713343650656196e-02",
            "-7.69187620504482999495e-02",
            "6.66107313738753120669e-02",
            "-5.83357013379057348645e-02",
            "4.97687799461593236017e-02",
            "-3.65315727442169155270e-02",
            "1.62858201153657823623e-02",
        ];
        let parsed = |text: &str| -> f64 { text.parse().expect("a decimal") };
        for (bits, text) in ATAN_HI.iter().zip(hi) {
            assert_eq!(*bits, parsed(text), "ATAN_HI against {text}");
        }
        for (bits, text) in ATAN_LO.iter().zip(lo) {
            assert_eq!(*bits, parsed(text), "ATAN_LO against {text}");
        }
        for (bits, text) in ATAN_POLY.iter().zip(poly) {
            assert_eq!(*bits, parsed(text), "ATAN_POLY against {text}");
        }
        assert_eq!(PI, parsed("3.1415926535897931160E+00"));
        assert_eq!(PI_LO, parsed("1.2246467991473531772E-16"));
    }

    #[test]
    fn the_constants_are_the_ones_core_names() {
        assert_eq!(PI, CORE_PI);
        assert_eq!(ATAN_HI[1], FRAC_PI_4);
        assert_eq!(ATAN_HI[3], FRAC_PI_2);
        // π/4 and π/2 differ from π by a power of two, so their remainders do
        // too, exactly.
        assert_eq!(ATAN_LO[1], PI_LO / 4.0);
        assert_eq!(ATAN_LO[3], PI_LO / 2.0);
    }

    /// Against `bc`, which knows nothing of any platform.
    #[test]
    fn atan2_matches_the_golden_table() {
        let rows = golden_rows().count();
        assert!(rows > 60, "the golden table has {rows} rows");
        for (y, x, want) in golden_rows() {
            let got = atan2(y, x);
            let error = ulps_apart(got, want);
            assert!(
                error <= MAX_ATAN2_ULP,
                "atan2({y:e}, {x:e}) = {got:e} is {error} ulp from bc's {want:e}"
            );
        }
    }

    /// The band where musl's cut differs from `e_atan2.c`'s: `x < 0` and
    /// `|y / x|` between `2^60` and `2^64` gives one ulp above `π/2`, and past
    /// `2^64` the outright `π/2` both return.
    #[test]
    fn musl_s_wide_band_is_one_ulp_above_half_pi() {
        let above = f64::from_bits(FRAC_PI_2.to_bits() + 1);
        for exponent in [61, 62, 63] {
            let y = f64::from_bits((1023 + exponent) << 52);
            assert_eq!(atan2(y, -1.0), above, "atan2(2^{exponent}, -1)");
            assert_eq!(atan2(-y, -1.0), -above, "atan2(-2^{exponent}, -1)");
        }
        let past = f64::from_bits((1023 + 65) << 52);
        assert_eq!(atan2(past, -1.0), FRAC_PI_2);
    }

    #[test]
    fn the_axes_and_the_diagonals_are_exact() {
        assert_eq!(atan2(0.0, 1.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(atan2(1.0, 0.0), FRAC_PI_2);
        assert_eq!(atan2(-1.0, 0.0), -FRAC_PI_2);
        assert_eq!(atan2(0.0, -1.0), CORE_PI);
        assert_eq!(atan2(-0.0, -1.0), -CORE_PI);
        assert_eq!(atan2(1.0, 1.0), FRAC_PI_4);
        assert_eq!(atan2(-1.0, 1.0), -FRAC_PI_4);
        assert_eq!(atan2(1.0, -1.0), 3.0 * FRAC_PI_4);
        assert_eq!(atan2(-1.0, -1.0), -3.0 * FRAC_PI_4);
        assert_eq!(atan(1.0), FRAC_PI_4);
        assert_eq!(atan(-1.0), -FRAC_PI_4);
    }

    /// C's `atan2` table (C17 F.10.1.4), case by case.
    #[test]
    fn the_signed_zeros_and_infinities_follow_c() {
        let inf = f64::INFINITY;
        let bits = |v: f64| v.to_bits();
        // atan2(±0, -0) is ±π and atan2(±0, +0) is ±0.
        assert_eq!(bits(atan2(0.0, -0.0)), bits(CORE_PI));
        assert_eq!(bits(atan2(-0.0, -0.0)), bits(-CORE_PI));
        assert_eq!(bits(atan2(0.0, 0.0)), bits(0.0));
        assert_eq!(bits(atan2(-0.0, 0.0)), bits(-0.0));
        // atan2(±0, x) for x < 0 and x > 0.
        assert_eq!(bits(atan2(0.0, -3.0)), bits(CORE_PI));
        assert_eq!(bits(atan2(-0.0, -3.0)), bits(-CORE_PI));
        assert_eq!(bits(atan2(0.0, 3.0)), bits(0.0));
        assert_eq!(bits(atan2(-0.0, 3.0)), bits(-0.0));
        // atan2(y, ±0) for y < 0 and y > 0.
        assert_eq!(atan2(-2.0, 0.0), -FRAC_PI_2);
        assert_eq!(atan2(-2.0, -0.0), -FRAC_PI_2);
        assert_eq!(atan2(2.0, 0.0), FRAC_PI_2);
        assert_eq!(atan2(2.0, -0.0), FRAC_PI_2);
        // atan2(±y, -∞) and atan2(±y, +∞) for finite y > 0.
        assert_eq!(bits(atan2(2.0, -inf)), bits(CORE_PI));
        assert_eq!(bits(atan2(-2.0, -inf)), bits(-CORE_PI));
        assert_eq!(bits(atan2(2.0, inf)), bits(0.0));
        assert_eq!(bits(atan2(-2.0, inf)), bits(-0.0));
        // atan2(±∞, x) for finite x.
        assert_eq!(atan2(inf, -2.0), FRAC_PI_2);
        assert_eq!(atan2(-inf, 2.0), -FRAC_PI_2);
        // atan2(±∞, ∓∞).
        assert_eq!(atan2(inf, -inf), 3.0 * FRAC_PI_4);
        assert_eq!(atan2(-inf, -inf), -3.0 * FRAC_PI_4);
        assert_eq!(atan2(inf, inf), FRAC_PI_4);
        assert_eq!(atan2(-inf, inf), -FRAC_PI_4);
        // atan(±∞) and atan(±0).
        assert_eq!(atan(inf), FRAC_PI_2);
        assert_eq!(atan(-inf), -FRAC_PI_2);
        assert_eq!(bits(atan(-0.0)), bits(-0.0));
    }

    #[test]
    fn a_nan_argument_is_a_nan_result() {
        for value in [0.0, 1.0, -1.0, f64::INFINITY, f64::NAN] {
            assert!(atan2(f64::NAN, value).is_nan(), "atan2(NaN, {value})");
            assert!(atan2(value, f64::NAN).is_nan(), "atan2({value}, NaN)");
        }
        assert!(atan(f64::NAN).is_nan());
    }

    /// The arguments the sweep and the digest visit: a log-spaced spread of
    /// magnitudes from about `2^-80` to `2^80` for each coordinate, in every
    /// sign combination, plus every pair of a uniform grid over `[-4, 4]`.
    fn for_each_sampled_point(mut visit: impl FnMut(f64, f64)) {
        const LOG_STEPS: i32 = 161;
        const GRID: i32 = 97;
        for i in 0..LOG_STEPS {
            for j in 0..LOG_STEPS {
                // Fixed significands keep the samples off powers of two.
                let y = 1.318_309_886 * power_of_two(i - 80);
                let x = 1.577_215_664 * power_of_two(j - 80);
                for (sy, sx) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                    visit(sy * y, sx * x);
                }
            }
        }
        let step = 8.0 / f64::from(GRID - 1);
        for i in 0..GRID {
            for j in 0..GRID {
                visit(-4.0 + step * f64::from(i), -4.0 + step * f64::from(j));
            }
        }
    }

    /// `2^e` for a normal-range `e`, written as its bits so the sample points
    /// need no platform `powi` to be the same everywhere.
    fn power_of_two(e: i32) -> f64 {
        let biased = u64::try_from(e + 1023).expect("a normal exponent");
        f64::from_bits(biased << 52)
    }

    /// The platform's `atan2` is the yardstick here and nowhere else: glibc's
    /// on the machine the bound was measured on, and whatever each CI target
    /// has, so a transcription slip shows as a gap on all of them. The
    /// platform's is itself only within an ulp of the truth, so the two may sit
    /// one further apart than [`MAX_ATAN2_ULP`].
    #[test]
    #[cfg_attr(
        miri,
        ignore = "an exhaustive arithmetic sweep with no unsafe; minutes under miri"
    )]
    fn atan2_tracks_the_platform() {
        let mut worst = 0;
        let mut worst_at = (0.0, 0.0);
        let mut visited = 0;
        for_each_sampled_point(|y, x| {
            visited += 1;
            let error = ulps_apart(atan2(y, x), y.atan2(x));
            if error > worst {
                worst = error;
                worst_at = (y, x);
            }
        });
        assert!(visited > 100_000, "the sweep covered only {visited} points");
        let bound = MAX_ATAN2_ULP + 1;
        assert!(
            worst <= bound,
            "atan2 is {worst} ulp from the platform's at {worst_at:?}, over {bound}"
        );
    }

    /// FNV-1a over the bits of [`atan2`] at every sampled point.
    fn digest() -> u64 {
        use core::hash::Hasher;
        let mut hash = crate::hash::Fnv1a64::new();
        for_each_sampled_point(|y, x| hash.write(&atan2(y, x).to_bits().to_le_bytes()));
        hash.finish()
    }

    /// **The same bits on every run and every target.** The constant was
    /// taken on x86-64 Linux; CI runs this test on aarch64 macOS and x86-64
    /// Windows too, where the platform's `atan2` differs from glibc's.
    #[test]
    #[cfg_attr(
        miri,
        ignore = "an exhaustive arithmetic sweep with no unsafe; minutes under miri"
    )]
    fn the_outputs_hash_to_the_pinned_digest() {
        /// The digest on x86-64 Linux, 2026-11-01.
        const PINNED: u64 = 0x4097_5875_d070_fe67;
        let first = digest();
        assert_eq!(first, digest(), "two runs over the same points disagree");
        assert_eq!(
            first, PINNED,
            "the outputs hash to {first:#018x}, not the pinned {PINNED:#018x}"
        );
    }
}
