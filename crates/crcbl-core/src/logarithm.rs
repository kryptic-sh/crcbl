//! `ln(1 + x)` in `f64`, computed without calling the platform's logarithm.
//!
//! The simulation's transcendentals are constructed in-engine, for the
//! reason [`crate::trig`] gives: the platform's `ln_1p` rounds differently on
//! glibc, Apple's libm, the MSVC runtime and the browser, and a replayed
//! projectile must reach the same bits on every one. This is what a
//! quadratic-drag flight step needs — the distance a body slowing as `1 /
//! (1 + k v t)` covers is `ln(1 + k v t) / (k v)`.
//!
//! # The construction
//!
//! `fdlibm`'s `s_log1p.c` (Sun Microsystems, 1993), transcribed: `1 + x` is
//! split into `2^k (1 + f)` with `√2/2 < 1 + f < √2`, the rounding of `1 + x`
//! is carried as a correction `c`, and `ln(1 + f)` is `f - f²/2 + s (f²/2 +
//! R(s²))` with `s = f / (2 + f)` and `R` a degree-7 polynomial in `s²`. Every
//! step is `+`, `-`, `*`, `/` or a bit operation, each of which IEEE-754
//! specifies exactly, so the result is the same bits on every target.
//!
//! `fdlibm` documents the error as under one unit in the last place; the
//! tests hold every golden argument to [`MAX_ULP`] of a reference computed
//! to 800 digits.

/// Units in the last place [`ln_1p`] may differ from the correctly rounded
/// result: `fdlibm`'s documented bound, which the golden table holds it to.
pub const MAX_ULP: u64 = 1;

// The constants are `fdlibm`'s, written as the bit patterns it gives them in,
// so no decimal-to-binary rounding stands between the source and these.

/// `ln 2`, split so that `k * LN2_HI` is exact for every exponent `k`.
const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000);
/// The rest of `ln 2`.
const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);
/// `2^54`.
const TWO54: f64 = 18_014_398_509_481_984.0;
/// `R(z)`'s coefficients, `Lp1` to `Lp7`.
const LP: [f64; 7] = [
    f64::from_bits(0x3fe5_5555_5555_5593),
    f64::from_bits(0x3fd9_9999_9997_fa04),
    f64::from_bits(0x3fd2_4924_9422_9359),
    f64::from_bits(0x3fcc_71c5_1d8e_78af),
    f64::from_bits(0x3fc7_4664_96cb_03de),
    f64::from_bits(0x3fc3_9a09_d078_c69f),
    f64::from_bits(0x3fc2_f112_df3e_5244),
];

/// The high 32 bits of `x`, as `fdlibm`'s signed `GET_HIGH_WORD`.
const fn high_word(x: f64) -> i32 {
    (x.to_bits() >> 32) as u32 as i32
}

/// `x` with its high 32 bits replaced by `high`, as `SET_HIGH_WORD`.
const fn with_high_word(x: f64, high: i32) -> f64 {
    f64::from_bits((x.to_bits() & 0xffff_ffff) | ((high as u32 as u64) << 32))
}

/// `ln(1 + x)`, accurate for `x` near zero where `ln(1.0 + x)` would lose
/// every digit `1 + x` rounded away, using only operations IEEE-754
/// specifies exactly.
///
/// `-1` gives negative infinity, anything below it and `NaN` give `NaN`,
/// positive infinity gives itself, and a zero keeps its sign.
#[must_use]
pub fn ln_1p(x: f64) -> f64 {
    let hx = high_word(x);
    let ax = hx & 0x7fff_ffff;
    let mut k = 1;
    let mut f = 0.0;
    let mut hu = 0;
    let mut c = 0.0;
    if hx < 0x3fda_827a {
        // x < 0.41422
        if ax >= 0x3ff0_0000 {
            // x <= -1
            return if x == -1.0 {
                f64::NEG_INFINITY
            } else {
                f64::NAN
            };
        }
        if ax < 0x3e20_0000 {
            // |x| < 2^-29
            if TWO54 + x > 0.0 && ax < 0x3c90_0000 {
                // |x| < 2^-54
                return x;
            }
            return x - x * x * 0.5;
        }
        // `fdlibm`'s `(int)0xbfd2bec4`: the high word of -0.2929.
        if hx > 0 || hx <= -0x402d_413c {
            // -0.2929 < x < 0.41422: no reduction
            k = 0;
            f = x;
            hu = 1;
        }
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    if k != 0 {
        let mut u;
        if hx < 0x4340_0000 {
            u = 1.0 + x;
            hu = high_word(u);
            k = (hu >> 20) - 1023;
            // The rounding error of 1 + x.
            c = if k > 0 { 1.0 - (u - x) } else { x - (u - 1.0) };
            c /= u;
        } else {
            u = x;
            hu = high_word(u);
            k = (hu >> 20) - 1023;
            c = 0.0;
        }
        hu &= 0x000f_ffff;
        if hu < 0x6_a09e {
            // u in [1, √2)
            u = with_high_word(u, hu | 0x3ff0_0000);
        } else {
            // u in [√2/2, 1)
            k += 1;
            u = with_high_word(u, hu | 0x3fe0_0000);
            hu = (0x0010_0000 - hu) >> 2;
        }
        f = u - 1.0;
    }
    let hfsq = 0.5 * f * f;
    let kf = f64::from(k);
    if hu == 0 {
        // |f| < 2^-20
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            c += kf * LN2_LO;
            return kf * LN2_HI + c;
        }
        let r = hfsq * (1.0 - (2.0 / 3.0) * f);
        if k == 0 {
            return f - r;
        }
        return kf * LN2_HI - ((r - (kf * LN2_LO + c)) - f);
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let r = z
        * (LP[0] + z * (LP[1] + z * (LP[2] + z * (LP[3] + z * (LP[4] + z * (LP[5] + z * LP[6]))))));
    if k == 0 {
        return f - (hfsq - s * (hfsq + r));
    }
    kf * LN2_HI - ((hfsq - (s * (hfsq + r) + (kf * LN2_LO + c))) - f)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One row per argument: its bits, then the bits of `ln(1 + x)` correctly
    /// rounded from Python's `decimal` at 800 digits (2026-10-12). The
    /// arguments cross every branch — the `2^-54` and `2^-29` cut-offs, the
    /// unreduced band `-0.2929 < x < 0.41422`, both halves of the reduction,
    /// `x ≥ 2^53`, down to just above `-1` and up to the largest `f64` — and
    /// sixty more are drawn at random.
    const GOLDEN: &str = "\
0000000000000001 0000000000000001
01a56e1fc2f8f359 01a56e1fc2f8f359
3c30000000000000 3c30000000000000
3c90000000000000 3c90000000000000
3c98000000000000 3c98000000000000
3e10000000000000 3e0fffffffc00000
3e20000000000000 3e1fffffff800000
3e2028f5c28f5c29 3e2028f5c24e12d7
3e45798ee2308c3a 3e45798ee0636111
3ee4f8b588e368f1 3ee4f8aea9ae7317
3f50624dd2f1a9fc 3f5060354f8c3ebf
3fb999999999999a 3fb8663f793c46c7
3fd2bd3c36113405 3fd06f94ecc02b71
bfd2bd3c36113405 bfd62c1a17840102
bfd2c083126e978d bfd630bc6e7aed9b
3fda826aa8eb4635 3fd62e386e3de810
3fda82be8bc169c2 3fd62e73bf2cd9da
3fe0000000000000 3fd9f323ecbf984c
bfe0000000000000 bfe62e42fefa39ef
3fe8000000000000 3fe1e85f5e7040d0
3ff0000000000000 3fe62e42fefa39ef
3ff8000000000000 3fed5240f0e0e078
4000000000000000 3ff193ea7aad030b
3ffb7e151628aed2 3ff0000000000000
4008000000000000 3ff62e42fefa39ef
4024000000000000 40032ee3b77f374c
408f400000000000 401ba2909ce4f864
4202a05f20000000 4037069e2aa3184e
4330000000000000 404205966f2b4f12
4340000000000000 40425e4f7b2737fa
4358000000000000 4042eaeecefca012
54b249ad2594c37d 406cc845b54b54f2
7e37e43c8800759c 4085963447f87fb5
7fefffffffffffff 40862e42fefa39ef
be45798ee2308c3a be45798ee3fdb764
bf50624dd2f1a9fc bf5064670d979b6f
bfb999999999999a bfbaf8e8210a415e
bfd0000000000000 bfd269621134db92
bfe6666666666666 bff34378fcbda720
bfeccccccccccccd c0026bb1bbb55516
bfefae147ae147ae c0126bb1bbb55515
bfeffffde7210be9 c02ba18a998fc064
bfeffffffffffffe c04205966f2b4f12
3fda827999fcef31 3fd62e42fefa39ee
3fda827999fcef33 3fd62e42fefa39f0
3eb0000000000000 3eafffff00000aab
3ff0000100000000 3fe62e43fefa35ef
beb0000000000000 beb0000080000555
3fda827999fcef34 3fd62e42fefa39f0
3e6235cc74f13ae1 3e6235cc6fc2caa8
44bc3de0c779bef7 404a9faab479ffad
37a6a48f863afca9 37a6a48f863afca9
385807e4ffcb94c1 385807e4ffcb94c1
453272e8e6b9e943 404d2ef28f8b750c
40733a6ec23600b8 4016edc9a75b4ae3
38c4888fd52eca6e 38c4888fd52eca6e
3b535a03b5c65d14 3b535a03b5c65d14
38762b61aa3ea55c 38762b61aa3ea55c
3b72bb0a69239067 3b72bb0a69239067
46637099cc9ca7ca 4051e5b077fb3c02
40ed81220af56d11 402604b2b1233686
38df523dcc934b4d 38df523dcc934b4d
40b50822bf8285aa 40212ecbb11be1d2
428596e4225b7508 403cb7f96164723c
3ee4be8d79b9faf4 3ee4be86c06d1d8b
43d8f4c409932b58 4045b5b744b393ab
39c0e0d755c984aa 39c0e0d755c984aa
46334fc4a5140737 4051602e72d03f75
45c551ad2785df55 40502ffb15d8c0fd
41828acbef1e6e31 403179e438e9b7c0
43651eb4f7fae508 4043334aee9de815
3f1f28cf9a57acd4 3f1f28563fb4c663
38aadf5bf0c36f61 38aadf5bf0c36f61
434bfc8ded8dccbc 4042a5e13754770d
39343505dfbcaa3b 39343505dfbcaa3b
3e20111bd60f70d8 3e20111bd5cee7b0
3f3b5a7afad1b09c 3f3b5904fa7fcbac
460b7ec7191b9e5e 4050f1b5749a1386
3d3896404ef8b8da 3d3896404ef8b7ac
3c7caf95466cb58f 3c7caf95466cb58f
383183e2816d314c 383183e2816d314c
3c7c56d90db6d5a6 3c7c56d90db6d5a6
39bae56f335ae04b 39bae56f335ae04b
3cf67749b1141412 3cf67749b1141402
4067aad56fba082e 4014fec6915ab7f5
3e72f17410b62028 3e72f174057f5fc0
4362348157c2a202 4043204859445596
38dd9a0ab00e31d9 38dd9a0ab00e31d9
4570ac8ae5fa1441 404e84e42dac2cd6
bfa4abb9388dac80 bfa519824e6a0f2d
bfd9a3fc6c29704e bfe06157d5d23bc0
bfee058265057629 c006446b259f192f
bfe63e48dfe75dcd bff301274d05b6e2
bfb40ba9c7359e68 bfb4dfb87c9517d0
bfd443fbe98c1560 bfd85e29b7ca2b0b
bfea62c6df289d87 bffbd8dd0b5b726b
bfaf1e7686cf8690 bfb00d6b9d7c8966
bfd73090b37378ee bfdccbf5f582f30d
bfe0f1abc029cc0e bfe820730cf2b655
bfe686cf09e9824c bff379d8b3c54ac3
bfc7af39157f6460 bfca30a8cb463521
bfebca723f96a453 c0003a4bafbc41c8
bfda60f0a9590bfe bfe10081f3482daf
bfe72cdee5efc7cb bff49c6128049795
bfdee9a1dea5b156 bfe51c84e08d43a9
bfc86ff6a9ab8460 bfcb1e04bebaffe3
bfb4c6276c4c62c0 bfb5aa5e283fc079
bfb63ebed28644c8 bfb74581412c794e
bfe052da205cf5e7 bfe6d5aa2708e569";

    /// The table's row count, so a truncated edit fails rather than checks
    /// less.
    const ROWS: usize = 109;

    /// How many representable values lie between two finite `f64`s of one
    /// sign.
    fn ulps(a: f64, b: f64) -> u64 {
        let (a, b) = (a.to_bits(), b.to_bits());
        a.max(b) - a.min(b)
    }

    /// **Every golden argument lands within [`MAX_ULP`] of the correctly
    /// rounded value.**
    #[test]
    fn every_golden_argument_is_within_an_ulp() {
        for row in GOLDEN.lines() {
            let mut fields = row
                .split(' ')
                .map(|field| f64::from_bits(u64::from_str_radix(field, 16).unwrap()));
            let (x, want) = (fields.next().unwrap(), fields.next().unwrap());
            let got = ln_1p(x);
            assert_eq!(got.is_sign_negative(), want.is_sign_negative(), "{x:e}");
            let off = ulps(got, want);
            assert!(
                off <= MAX_ULP,
                "ln_1p({x:e}) = {got:e}, want {want:e}: {off} ulp"
            );
        }
        assert_eq!(GOLDEN.lines().count(), ROWS, "the table was cut short");
    }

    /// The edges: zeros keep their sign, `-1` is negative infinity, below it
    /// and `NaN` are `NaN`, infinity is itself, and below `2^-54` the result
    /// is the argument.
    #[test]
    fn the_edges_are_exact() {
        assert_eq!(ln_1p(0.0).to_bits(), 0.0_f64.to_bits());
        assert_eq!(ln_1p(-0.0).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(ln_1p(-1.0), f64::NEG_INFINITY);
        assert!(ln_1p(-1.5).is_nan());
        assert!(ln_1p(f64::NAN).is_nan());
        assert!(ln_1p(f64::NEG_INFINITY).is_nan());
        assert_eq!(ln_1p(f64::INFINITY), f64::INFINITY);
        assert_eq!(ln_1p(1e-20), 1e-20);
    }

    /// FNV-1a over the outputs at a sweep of arguments built by halving and
    /// scaling, which IEEE-754 makes exact, so the arguments are the same on
    /// every target as well as the outputs.
    fn digest() -> u64 {
        use core::hash::Hasher;
        let mut hash = crate::hash::Fnv1a64::new();
        let mut magnitude = 1e30_f64;
        while magnitude > 1e-30 {
            for x in [magnitude, -magnitude.min(0.999_999), magnitude * 0.7] {
                hash.write(&ln_1p(x).to_bits().to_le_bytes());
            }
            magnitude *= 0.999;
        }
        hash.finish()
    }

    /// **The same bits on every target.** CI runs this on x86-64 Linux and
    /// Windows and on aarch64 macOS, so a target whose arithmetic differs in
    /// one bit of one output fails here rather than in a replay.
    #[test]
    fn the_outputs_hash_to_the_pinned_digest() {
        /// The digest on x86-64 Windows, 2026-10-12.
        const PINNED: u64 = 0x824e_b588_1746_9c25;
        let first = digest();
        assert_eq!(first, digest(), "two runs disagree");
        assert_eq!(first, PINNED, "the outputs hash to {first:#018x}");
    }
}
