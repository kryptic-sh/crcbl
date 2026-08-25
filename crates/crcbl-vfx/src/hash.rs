//! Stateless per-particle randomness.
//!
//! `docs/plan/20-particles.md` spells the requirement out: "randomness =
//! per-particle hash of (seed, index) — stateless, replayable, which makes
//! golden-frame testing of particles possible (fixed seed + fixed time step =
//! identical frames)". Every random value in this crate comes from here, and
//! nothing in it holds generator state.
//!
//! # Why a hash and not the engine's PRNG
//!
//! `crcbl-rand`'s `Rng` is a ChaCha8 stream. A stream is reproducible from
//! its seed only while every consumer draws from it in the same order, so the
//! moment a particle retires early — or the pool clamps a spawn, or the effects
//! are stepped in a different order — every later particle in the frame gets
//! different values. A hash of (seed, index) has no such coupling: particle
//! *k* of effect *s* draws the same numbers whatever else happened, on the CPU
//! now and in the compute pass this is staged towards, where there is no shared
//! stream to draw from in the first place.
//!
//! # The hash
//!
//! [`pcg3d`] is the three-word PCG hash from Mark Jarzynski and Marc Olano,
//! *Hash Functions for GPU Rendering*, Journal of Computer Graphics Techniques
//! 9(3), 2020 — the survey that measured the usual shader hashes against the
//! statistical suites and recommended this family in their place. It takes
//! three 32-bit words and returns three, which is exactly the shape wanted
//! here: (effect seed, particle index, stream) in, three independent values
//! out.
//!
//! Its first stage is an ordinary linear congruential step with the
//! multiplier and increment of *Numerical Recipes*' `ranqd1`; the two
//! stages after it are the paper's own permute-and-mix. The whole function is
//! integer arithmetic with wrapping multiplies, so it evaluates identically on
//! every target — unlike the `sin`-based hashes it replaces, and unlike the
//! rest of this crate's `f32` maths.

use std::num::Wrapping;

/// The multiplier of the LCG stage: *Numerical Recipes*' `ranqd1`, which is
/// what Jarzynski and Olano's listing uses.
const LCG_MUL: u32 = 1_664_525;

/// The increment of that same LCG.
const LCG_ADD: u32 = 1_013_904_223;

/// Three independent 32-bit words hashed from three.
///
/// The `pcg3d` of *Hash Functions for GPU Rendering*, transcribed verbatim:
///
/// ```text
/// v = v * 1664525u + 1013904223u
/// v.x += v.y * v.z;  v.y += v.z * v.x;  v.z += v.x * v.y
/// v ^= v >> 16u
/// v.x += v.y * v.z;  v.y += v.z * v.x;  v.z += v.x * v.y
/// ```
///
/// Note that the second mixing round reads words the same round has already
/// written — `v.y` is added to using the *new* `v.x`. That is the published
/// order and it is load-bearing; evaluating the three in parallel from the old
/// values is a different, weaker function.
///
/// Every multiply and add wraps. In the paper this is a shader operating on
/// `uvec3` where wrapping is the only behaviour there is; here [`Wrapping`]
/// says so, because a plain `*` would panic in a debug build.
pub fn pcg3d(v: [u32; 3]) -> [u32; 3] {
    let (mul, add) = (Wrapping(LCG_MUL), Wrapping(LCG_ADD));
    let mut x = Wrapping(v[0]) * mul + add;
    let mut y = Wrapping(v[1]) * mul + add;
    let mut z = Wrapping(v[2]) * mul + add;

    x += y * z;
    y += z * x;
    z += x * y;

    x ^= Wrapping(x.0 >> 16);
    y ^= Wrapping(y.0 >> 16);
    z ^= Wrapping(z.0 >> 16);

    x += y * z;
    y += z * x;
    z += x * y;

    [x.0, y.0, z.0]
}

/// A hashed word mapped onto `[0, 1)`.
///
/// The top 24 bits over 2²⁴, which is the mapping that costs nothing in
/// accuracy: an `f32` has 24 bits of significand, so every value this can
/// produce is exact and the spacing is uniform. Taking all 32 bits and dividing
/// by `u32::MAX` instead would round, and would reach `1.0` — a value callers
/// interpolating over `[0, 1)` do not want.
pub fn unit(word: u32) -> f32 {
    const SCALE: f32 = 1.0 / (1u32 << 24) as f32;
    (word >> 8) as f32 * SCALE
}

/// A hashed word mapped onto `[lo, hi)`, or onto `lo` when the two are equal.
///
/// The endpoints are not sorted: `range(w, 2.0, 1.0)` walks downwards, which is
/// a caller's business and not an error.
pub fn range(word: u32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * unit(word)
}
