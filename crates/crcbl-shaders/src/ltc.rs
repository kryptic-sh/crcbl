//! The linearly transformed cosine fit of the GGX lobe: the table an area
//! light's specular highlight is shaped by.
//!
//! Heitz, Dupuy, Hill and Neubelt, *Real-Time Polygonal-Light Shading with
//! Linearly Transformed Cosines* (SIGGRAPH 2016). The idea in one sentence: a
//! clamped cosine distribution has a **closed form** over a spherical polygon,
//! and a GGX lobe is close enough to a linear transform of one that fitting the
//! transform per `(N·V, roughness)` turns an area light's specular integral
//! into a 3×3 matrix, four normalises and a per-edge rational — no sampling and
//! no march.
//!
//! So this module holds two things: the fit, which runs at cook time and is
//! full of transcendentals; and the committed result, `tables/ltc.bin`, which
//! is what the shader reads. That split is [`crate::dfg`]'s exactly, and for
//! the same reason — `docs/plan/44-lighting.md`'s rule that no transcendental
//! may reach a colour, with the cooked table as one of the two ways out of it.
//!
//! # The grid is `dfg`'s grid
//!
//! Both tables are indexed by `(N·V, roughness)`, both are [`crate::dfg::DFG_SIZE`]
//! square, and both are sampled at texel centres through [`crate::dfg::axis_value`].
//! That is not a coincidence to be tidied away later: `mesh.slang` reads both
//! for the same fragment, so one pair of texel coordinates and one set of
//! bilinear weights serves both, and a shape change to one is a shape change to
//! the other by construction.
//!
//! It is also why the second half of the paper's pair — the lobe's magnitude
//! and its Fresnel term, which scale the polygon integral — is **not** cooked
//! here. Those two numbers are `∫ f` and `∫ f (1 − V·H)^5`, which is exactly
//! what [`crate::dfg`] already holds: its `scale` is `∫ f (1 − (1 − V·H)^5)`
//! and its `bias` is `∫ f (1 − V·H)^5`, so the paper's
//! `f0 · magnitude + (1 − f0) · fresnel` is the same number as Karis's
//! `f0 · scale + bias`. One integral, one table, and no second copy to drift.
//!
//! # What a texel holds
//!
//! The inverse of the fitted transform, in the shading frame where the normal
//! is `+z` and the view lies in the `+xz` half-plane. In that frame the matrix
//! has four non-zero entries off the middle:
//!
//! ```text
//! M⁻¹ = | a  0  b |
//!       | 0  1  0 |
//!       | c  0  d |
//! ```
//!
//! and `(a, b, c, d)` is the texel. **The middle entry is one because the
//! polygon integral is invariant to a positive scale of `M⁻¹`** — every
//! transformed vertex is normalised before it is integrated — so the fit's
//! fifth number is free and is spent normalising the other four. That is the
//! paper's own packing.
//!
//! Regenerate or verify with the tool that owns it:
//!
//! ```text
//! cargo run -p crcbl-shaders --example cook-ltc            # regenerate
//! cargo run -p crcbl-shaders --example cook-ltc -- --check # verify only
//! ```

use crate::dfg::{DFG_SIZE, axis_value};

/// Texels along each axis of the table — [`crate::dfg::DFG_SIZE`], because the
/// two tables share a grid and the module header says why.
pub const LTC_SIZE: usize = DFG_SIZE;

/// Bytes one entry occupies: four little-endian `f32`s, `a`, `b`, `c` then `d`.
///
/// `f32` rather than the half floats the GPU image holds, for
/// [`crate::dfg::DFG_ENTRY_BYTES`]' reason: the committed artifact is the
/// integrator's own output, and the rounding to a GPU format happens once, in
/// [`texels`], where a test can measure it.
pub const LTC_ENTRY_BYTES: usize = 16;

/// The committed table's exact length.
///
/// The artifact is typed `&[u8; LTC_BYTES]` where it is included, so a table of
/// the wrong size fails to compile rather than being caught by a test.
pub const LTC_BYTES: usize = LTC_SIZE * LTC_SIZE * LTC_ENTRY_BYTES;

/// Samples along each axis of the stratified square the fit's error integral
/// and its average direction are estimated over.
///
/// The estimator draws this many squared from each of two densities — the lobe
/// and the fitted distribution — and weights them against each other, so a
/// texel costs `2 · LTC_SAMPLES²` lobe evaluations per objective call. A
/// stratified grid rather than a sequence: there is no random number anywhere
/// in the cook, so the table is a function of this crate's source and nothing
/// else.
pub const LTC_SAMPLES: u32 = 32;

/// How many Nelder-Mead steps the fit is allowed per texel.
///
/// Every texel but the first starts from its neighbour's answer, so the
/// simplex opens a short way from the minimum and this is a ceiling rather
/// than a working figure — the fit reaches `FIT_TOLERANCE` long before it in
/// every row. It is here so the cook cannot run unboundedly on a texel where
/// the objective is flat.
pub const LTC_FIT_STEPS: usize = 64;

/// The smallest roughness the fit is run at, matching `mesh.slang`'s
/// `MIN_ROUGHNESS`.
///
/// The shader clamps a material's roughness to this before it reads either
/// table, so the rows below it can never be sampled — and fitting a lobe an
/// order of magnitude sharper than anything that will be asked for is how a
/// Monte Carlo fit spends its samples on noise. Every row below this roughness
/// therefore holds the fit at exactly this roughness, which is the value the
/// shader would have clamped to anyway.
pub const LTC_MIN_ROUGHNESS: f64 = 0.045;

/// The committed table, `tables/ltc.bin`.
///
/// Not a path anything resolves at run time: it is in the binary, exactly as
/// the compiled shaders are, so there is no file for a deployment to lose.
const TABLE: &[u8; LTC_BYTES] = include_bytes!("../tables/ltc.bin");

/// The committed entry at `(n_dot_v_index, roughness_index)`, as `(a, b, c, d)`.
///
/// # Panics
///
/// If either index is at or past [`LTC_SIZE`].
#[must_use]
pub fn entry(n_dot_v_index: usize, roughness_index: usize) -> [f32; 4] {
    assert!(
        n_dot_v_index < LTC_SIZE && roughness_index < LTC_SIZE,
        "({n_dot_v_index}, {roughness_index}) is outside a {LTC_SIZE}-square table"
    );
    let at = (roughness_index * LTC_SIZE + n_dot_v_index) * LTC_ENTRY_BYTES;
    let mut out = [0.0f32; 4];
    for (slot, value) in out.iter_mut().enumerate() {
        let word = at + slot * 4;
        *value = f32::from_le_bytes([
            TABLE[word],
            TABLE[word + 1],
            TABLE[word + 2],
            TABLE[word + 3],
        ]);
    }
    out
}

/// The committed table's bytes, for a caller uploading it to a device.
///
/// Row-major, `roughness` slow and `N·V` fast, which is the order a 2D image
/// upload expects and the order [`entry`] indexes in.
#[must_use]
pub const fn bytes() -> &'static [u8; LTC_BYTES] {
    TABLE
}

/// Bytes one texel of [`texels`] occupies: an `Rgba16Float` quad.
///
/// **Half floats rather than the fixed point [`crate::dfg::pair_texels`]
/// uses**, and the difference is the range rather than a preference. That
/// table stores shares of arriving light, which live in `[0, 1]` where a
/// fixed-point step beats half precision everywhere. These four numbers are
/// matrix entries: `a` and `d` run from a hair above zero at a mirror to
/// several at a rough surface seen edge-on, `b` and `c` change sign, and there
/// is no interval to divide into 65 536 parts. Half floats spend their
/// precision relatively rather than absolutely, which is what a matrix entry
/// wants, and `Rgba16Float` is a filterable sampled format on every target this
/// engine opens.
///
/// The conversion is [`half_bits`], which is integer arithmetic on the IEEE-754
/// fields and therefore the same on every machine that cooks the image — the
/// property the committed `f32` artifact would lose if it were stored this way.
pub const LTC_TEXEL_BYTES: usize = 8;

/// The length of [`texels`], one [`LTC_TEXEL_BYTES`] per texel of an
/// [`LTC_SIZE`]-square image.
pub const LTC_TEXELS_BYTES: usize = LTC_SIZE * LTC_SIZE * LTC_TEXEL_BYTES;

/// The committed table encoded for upload as an `Rgba16Float` image, in
/// [`entry`]'s order — row-major, `roughness` slow and `N·V` fast.
#[must_use]
pub fn texels() -> Vec<u8> {
    let mut texels = Vec::with_capacity(LTC_TEXELS_BYTES);
    for roughness in 0..LTC_SIZE {
        for n_dot_v in 0..LTC_SIZE {
            for value in entry(n_dot_v, roughness) {
                texels.extend_from_slice(&half_bits(value).to_le_bytes());
            }
        }
    }
    texels
}

/// `value` as the sixteen bits of an IEEE-754 binary16, rounded to nearest with
/// ties to even.
///
/// Written out rather than taken from a dependency, and it is integer
/// arithmetic on the binary32 fields rather than any kind of numeric
/// approximation: the sign moves down, the exponent is rebiased from 127 to 15,
/// and the mantissa is shifted by thirteen bits with the round-to-nearest-even
/// correction added before the shift. Overflow saturates to infinity and the
/// subnormal range below `2⁻¹⁴` is produced by the same shift on a mantissa
/// with the implicit one put back.
///
/// [`half_value`] is the inverse, and `a_half_round_trips_every_value_the_table_holds`
/// is what holds the pair together over the numbers this module actually
/// stores.
#[must_use]
pub fn half_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x007f_ffff;

    if exponent == 0xff {
        // Infinity keeps its sign; a NaN keeps a non-zero mantissa so it stays
        // one rather than becoming an infinity.
        let payload = if mantissa == 0 { 0 } else { 0x0200 };
        return sign | 0x7c00 | payload;
    }

    // The binary16 exponent, before the rounding below can carry into it.
    let shifted = exponent - 127 + 15;
    if shifted >= 0x1f {
        return sign | 0x7c00;
    }
    if shifted <= 0 {
        if shifted < -10 {
            // Below half of the smallest subnormal, so it rounds to zero.
            return sign;
        }
        // Subnormal: put the implicit one back and shift it down into place,
        // rounding to nearest with ties to even on the bits that fall off.
        let with_implicit = mantissa | 0x0080_0000;
        let shift = (14 - shifted) as u32;
        let kept = with_implicit >> shift;
        let half = 1u32 << (shift - 1);
        let dropped = with_implicit & ((1u32 << shift) - 1);
        let round = u32::from(dropped > half || (dropped == half && (kept & 1) == 1));
        return sign | (kept + round) as u16;
    }

    let kept = (shifted as u32) << 10 | (mantissa >> 13);
    let dropped = mantissa & 0x1fff;
    let round = u32::from(dropped > 0x1000 || (dropped == 0x1000 && (kept & 1) == 1));
    sign | (kept + round) as u16
}

/// The number an IEEE-754 binary16 holds, as an `f32`.
///
/// [`half_bits`]' inverse, and the value the GPU hands a fragment when it loads
/// a texel of the image [`texels`] wrote. Exact in both directions: every
/// binary16 is a binary32, so this loses nothing and a test can compare a
/// round trip against the quantisation step rather than against a tolerance
/// somebody chose.
#[must_use]
pub fn half_value(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = u32::from(bits & 0x03ff);

    if exponent == 0x1f {
        return f32::from_bits(sign | 0x7f80_0000 | (mantissa << 13));
    }
    if exponent == 0 {
        if mantissa == 0 {
            return f32::from_bits(sign);
        }
        // Subnormal: shift the leading one up into the implicit position and
        // pay for it in the exponent. A binary16 subnormal is
        // `mantissa · 2⁻²⁴`, so once the leading one sits at bit 10 the value
        // is `1.f · 2^(-14 - shifts)` and the binary32 bias puts that at
        // `113 - shifts`.
        let mut exponent = 113i32;
        let mut mantissa = mantissa;
        while mantissa & 0x0400 == 0 {
            mantissa <<= 1;
            exponent -= 1;
        }
        let mantissa = (mantissa & 0x03ff) << 13;
        return f32::from_bits(sign | ((exponent as u32) << 23) | mantissa);
    }
    f32::from_bits(sign | (((exponent - 15 + 127) as u32) << 23) | (mantissa << 13))
}

/// The table sampled bilinearly, the way the shader's hand-written filter reads
/// it.
///
/// [`crate::dfg::sample`]'s addressing exactly — `value · size − 0.5`, both ends
/// clamped to the edge texel — because the two tables share a grid and the
/// shader computes those weights once for both.
#[must_use]
pub fn sample(n_dot_v: f32, roughness: f32) -> [f32; 4] {
    let axis = |value: f32| {
        let scaled = value.clamp(0.0, 1.0) * LTC_SIZE as f32 - 0.5;
        let low = scaled.floor().clamp(0.0, (LTC_SIZE - 1) as f32);
        let high = (low + 1.0).min((LTC_SIZE - 1) as f32);
        (low as usize, high as usize, (scaled - low).clamp(0.0, 1.0))
    };
    let (x0, x1, fx) = axis(n_dot_v);
    let (y0, y1, fy) = axis(roughness);
    let mut out = [0.0f32; 4];
    for (channel, slot) in out.iter_mut().enumerate() {
        let top = entry(x0, y0)[channel] * (1.0 - fx) + entry(x1, y0)[channel] * fx;
        let bottom = entry(x0, y1)[channel] * (1.0 - fx) + entry(x1, y1)[channel] * fx;
        *slot = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// The inverse transform `(a, b, c, d)` stands for, as a row-major 3×3 in the
/// shading frame.
///
/// The middle row is the identity's, which the module header says why: the
/// integral is invariant to a positive scale of this matrix, so the fit's fifth
/// number is spent normalising the four that are stored.
#[must_use]
pub fn inverse_transform(entry: [f32; 4]) -> [[f32; 3]; 3] {
    [
        [entry[0], 0.0, entry[1]],
        [0.0, 1.0, 0.0],
        [entry[2], 0.0, entry[3]],
    ]
}

/// The `π` this engine's two lobes folded out of themselves, put back once at
/// the end of a polygon integral.
///
/// `docs/plan/44-lighting.md` records the convention: neither the Lambert term
/// nor the specular one carries its `1 / π`, because a light's intensity has
/// absorbed it. The published edge fit below carries a `1 / (2π)` instead — it
/// is written to hand a shader the Lambertian *response* `E / π` directly — so
/// an engine that wants the irradiance `E` itself multiplies it back. One
/// constant, in one place, rather than three altered coefficients nobody could
/// check against the paper.
pub const LOBE_PI: f32 = std::f32::consts::PI;

/// `θ / (2π sin θ)` for `cos θ = cosine`, by the rational fit the paper's own
/// shader uses in place of an `acos`.
///
/// Transcribed from Hill's `LTC_Evaluate`. It matters twice over here: the
/// `acos` and `sin` it replaces are exactly the transcendentals
/// `docs/plan/44-lighting.md` refuses in a term that reaches a colour, and the
/// replacement is multiplies, one divide and one reciprocal square root — every
/// one of them an operation IEEE-754 specifies, which is the same ground
/// [`crate::fog`]'s exponential stands on.
///
/// The two branches are the fit's own: the rational covers `cos θ ≥ 0`, and the
/// obtuse half is reached through the identity `θ / sin θ = π / sin θ − (π − θ) / sin θ`,
/// which is what the `0.5 · rsqrt(1 − x²)` term is once the `2π` is folded in.
#[must_use]
pub fn edge_weight(cosine: f32) -> f32 {
    let y = cosine.abs();
    let numerator = 0.854_398_5 + (0.496_515_5 + 0.014_520_6 * y) * y;
    let denominator = 3.417_594 + (4.161_672_4 + y) * y;
    let fit = numerator / denominator;
    if cosine > 0.0 {
        fit
    } else {
        0.5 / (1.0 - cosine * cosine).max(1e-7).sqrt() - fit
    }
}

/// The clamped-cosine integral `∫ max(0, cos θ) dω` over the spherical polygon
/// the four `corners` subtend at the origin, with the receiver's normal at
/// `+z`.
///
/// The corners are **relative to the shading point** and already carry whatever
/// transform is being integrated under: the identity for the diffuse term, and
/// [`inverse_transform`] for the specular one, which is the whole of what a
/// linearly transformed cosine is.
///
/// A polygon covering the whole hemisphere returns `π`; one entirely below the
/// horizon returns zero, and one straddling it is clipped rather than clamped.
/// `the_polygon_integral_agrees_with_a_hemisphere_quadrature` is what says the
/// clip, the winding and the edge fit all agree with a brute-force sweep of the
/// same hemisphere, straddling quads included.
///
/// **One-sided**: a negative sum is a polygon wound the other way round, which
/// means the receiver is behind the light, and it returns zero rather than its
/// absolute value. `crcbl_render::RectLight` documents which way that is.
#[must_use]
pub fn polygon_irradiance(corners: [[f32; 3]; 4]) -> f32 {
    let (mut points, count) = clip_to_horizon(corners);
    if count == 0 {
        return 0.0;
    }
    for point in &mut points[..count] {
        let length = (point[0] * point[0] + point[1] * point[1] + point[2] * point[2])
            .max(1e-12)
            .sqrt();
        for value in point.iter_mut() {
            *value /= length;
        }
    }

    let mut sum = 0.0f32;
    for index in 0..count {
        let first = points[index];
        let second = points[(index + 1) % count];
        let cosine = first[0] * second[0] + first[1] * second[1] + first[2] * second[2];
        // Only the `z` of the cross product is wanted: the receiver's normal is
        // `+z` in this frame, and the edge term is that cross dotted with it.
        let cross_z = first[0] * second[1] - first[1] * second[0];
        sum += cross_z * edge_weight(cosine.clamp(-1.0, 1.0));
    }
    sum.max(0.0) * LOBE_PI
}

/// The four corners clipped against the `z > 0` half-space, and how many
/// survived.
///
/// A quad crossing the horizon becomes a triangle, a quad or a pentagon, which
/// is why the array is five long. Transcribed from the paper's
/// `ClipQuadToHorizon`: the configuration is the four sign bits, and each case
/// replaces the corners below the horizon with the points where their edges
/// cross it. Two of the sixteen configurations — opposite corners above, the
/// other two below — cannot happen for a planar quad and return nothing.
fn clip_to_horizon(corners: [[f32; 3]; 4]) -> ([[f32; 3]; 5], usize) {
    let mut l = [[0.0f32; 3]; 5];
    l[..4].copy_from_slice(&corners);

    // Where an edge from `a` to `b` crosses `z = 0`, unnormalised — the same
    // `-b.z · a + a.z · b` the reference writes, which is the crossing scaled
    // by the two heights and therefore needs no divide.
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            -b[2] * a[0] + a[2] * b[0],
            -b[2] * a[1] + a[2] * b[1],
            -b[2] * a[2] + a[2] * b[2],
        ]
    };

    let mut config = 0usize;
    for (bit, corner) in l[..4].iter().enumerate() {
        if corner[2] > 0.0 {
            config |= 1 << bit;
        }
    }

    let count = match config {
        0 => 0,
        1 => {
            l[1] = cross(l[0], l[1]);
            l[2] = cross(l[0], l[3]);
            3
        }
        2 => {
            l[0] = cross(l[1], l[0]);
            l[2] = cross(l[1], l[2]);
            3
        }
        3 => {
            l[2] = cross(l[1], l[2]);
            l[3] = cross(l[0], l[3]);
            4
        }
        4 => {
            l[0] = cross(l[2], l[3]);
            l[1] = cross(l[2], l[1]);
            3
        }
        6 => {
            l[0] = cross(l[1], l[0]);
            l[3] = cross(l[2], l[3]);
            4
        }
        7 => {
            l[4] = cross(l[0], l[3]);
            l[3] = cross(l[2], l[3]);
            5
        }
        8 => {
            l[0] = cross(l[3], l[0]);
            l[1] = cross(l[3], l[2]);
            l[2] = l[3];
            3
        }
        9 => {
            l[1] = cross(l[0], l[1]);
            l[2] = cross(l[3], l[2]);
            4
        }
        11 => {
            l[4] = l[3];
            l[3] = cross(l[2], l[3]);
            l[2] = cross(l[1], l[2]);
            5
        }
        12 => {
            l[1] = cross(l[2], l[1]);
            l[0] = cross(l[3], l[0]);
            4
        }
        13 => {
            l[4] = l[3];
            l[3] = l[2];
            l[2] = cross(l[1], l[2]);
            l[1] = cross(l[0], l[1]);
            5
        }
        14 => {
            l[4] = cross(l[3], l[0]);
            l[0] = cross(l[1], l[0]);
            5
        }
        15 => 4,
        // 5 and 10 are two opposite corners above the horizon and the other two
        // below, which a planar quad cannot be in.
        _ => 0,
    };
    (l, count)
}

/// A row-major 3×3 in `f64`, which is the only matrix this module's fit needs.
type Mat3 = [[f64; 3]; 3];

/// How small the simplex has to get before the fit stops.
///
/// Hill's own figure. The objective is an `L3` error between two lobes, so this
/// is a bound on the *parameters* rather than on the error: past it the matrix
/// no longer moves by as much as the half float the image stores can hold.
const FIT_TOLERANCE: f64 = 1e-5;

/// How far the first simplex opens from the warm start, in each parameter.
///
/// Hill's `epsilon`. Every texel but the first starts from a neighbour that was
/// already fitted, so the minimum is near and a wide opening would only spend
/// steps walking back.
const FIT_STEP: f64 = 0.05;

/// The smallest a fitted scale may be, so the transform stays invertible.
const MIN_SCALE: f64 = 1e-7;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = dot(v, v).max(1e-300).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

fn apply(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [dot(m[0], v), dot(m[1], v), dot(m[2], v)]
}

fn multiply(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0f64; 3]; 3];
    for (row, slot) in out.iter_mut().enumerate() {
        for (column, value) in slot.iter_mut().enumerate() {
            *value = a[row][0] * b[0][column] + a[row][1] * b[1][column] + a[row][2] * b[2][column];
        }
    }
    out
}

fn determinant(m: &Mat3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn invert(m: &Mat3) -> Mat3 {
    let det = determinant(m);
    let scale = 1.0 / if det.abs() < 1e-300 { 1e-300 } else { det };
    let mut out = [[0.0f64; 3]; 3];
    for (row, slot) in out.iter_mut().enumerate() {
        for (column, value) in slot.iter_mut().enumerate() {
            // The cofactor of (column, row), which is the adjugate's (row,
            // column) — the transpose is what makes this the inverse rather
            // than the cofactor matrix.
            let (r0, r1) = ((column + 1) % 3, (column + 2) % 3);
            let (c0, c1) = ((row + 1) % 3, (row + 2) % 3);
            *value = (m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]) * scale;
        }
    }
    out
}

/// The GGX lobe this engine shades with, evaluated **with its cosine folded
/// in**, and the density the sampler below draws from.
///
/// `D · G / (4 N·V)` is `f · N·L`: the microfacet denominator's `N·L` cancels
/// the cosine, which is what makes this the function a linearly transformed
/// cosine is fitted to. `G` is Smith height-correlated, spelled as
/// `1 / (1 + Λv + Λl)` — the same visibility `ggx_lobe` writes as
/// `0.5 / (λv + λl)`, so the table describes the lobe this engine actually
/// shades with rather than the one a reference implementation ships.
fn ggx_eval(view: [f64; 3], light: [f64; 3], alpha: f64) -> (f64, f64) {
    if view[2] <= 0.0 || light[2] <= 0.0 {
        return (0.0, 0.0);
    }
    let lambda = |cosine: f64| {
        if cosine >= 1.0 {
            return 0.0;
        }
        // `1 / (α tan θ)` with `tan θ = sqrt(1 - c²) / c`, written without the
        // trigonometry the identity removes.
        let tangent = (1.0 - cosine * cosine).max(0.0).sqrt() / cosine;
        let a = 1.0 / (alpha * tangent).max(1e-300);
        0.5 * (-1.0 + (1.0 + 1.0 / (a * a)).sqrt())
    };
    let lambda_v = lambda(view[2]);
    let lambda_l = lambda(light[2]);
    let geometry = 1.0 / (1.0 + lambda_v + lambda_l);

    let half = normalize([view[0] + light[0], view[1] + light[1], view[2] + light[2]]);
    let slope_x = half[0] / half[2];
    let slope_y = half[1] / half[2];
    let mut distribution = 1.0 / (1.0 + (slope_x * slope_x + slope_y * slope_y) / (alpha * alpha));
    distribution *= distribution;
    distribution /= std::f64::consts::PI * alpha * alpha * half[2].powi(4);

    let pdf = (distribution * half[2] / (4.0 * dot(view, half))).abs();
    (distribution * geometry / (4.0 * view[2]), pdf)
}

/// A direction drawn from the lobe's own distribution, by sampling a slope and
/// reflecting the view about the half-vector it names.
fn ggx_sample(view: [f64; 3], alpha: f64, u1: f64, u2: f64) -> [f64; 3] {
    let phi = std::f64::consts::TAU * u1;
    let radius = alpha * (u2 / (1.0 - u2).max(1e-300)).sqrt();
    let half = normalize([radius * phi.cos(), radius * phi.sin(), 1.0]);
    let along = 2.0 * dot(half, view);
    [
        -view[0] + along * half[0],
        -view[1] + along * half[1],
        -view[2] + along * half[2],
    ]
}

/// One fitted lobe: the transform, its inverse, and the two scalars the split
/// Fresnel needs.
#[derive(Clone, Copy, Debug)]
struct Lobe {
    magnitude: f64,
    m11: f64,
    m22: f64,
    m13: f64,
    frame: Mat3,
    transform: Mat3,
    inverse: Mat3,
    det: f64,
}

impl Lobe {
    /// Rebuilds [`Lobe::transform`] and its inverse from the three parameters
    /// and the frame.
    ///
    /// `M = F · S`, where `F`'s columns are the frame's axes and `S` scales the
    /// cosine lobe by `m11` across the view plane, by `m22` across the other
    /// axis, and tilts its wide axis out of the frame's plane by `m13`. That is
    /// the paper's three-parameter isotropic form: a fourth would let the lobe
    /// rotate about the normal, which an isotropic BRDF never asks for.
    ///
    /// **The shear is on the `x` axis rather than on the pole**, which is the
    /// one place this transcription had to choose. Both placements fit the lobe
    /// about equally well — measured, the pointwise disagreement with the lobe
    /// differs by a part in twenty — but tilting the pole leaves an inverse
    /// whose largest entry reaches 102, where tilting the wide axis keeps every
    /// entry under 2. A well-conditioned inverse is what the half floats of
    /// [`texels`] want, and it is what makes an entry's precision a relative
    /// question rather than an absolute one.
    fn update(&mut self) {
        let scale: Mat3 = [
            [self.m11, 0.0, 0.0],
            [0.0, self.m22, 0.0],
            [self.m13, 0.0, 1.0],
        ];
        self.transform = multiply(&self.frame, &scale);
        self.inverse = invert(&self.transform);
        self.det = determinant(&self.transform).abs();
    }

    /// The fitted distribution at `light`, on the same scale [`ggx_eval`]
    /// returns.
    fn eval(&self, light: [f64; 3]) -> f64 {
        let original = normalize(apply(&self.inverse, light));
        let back = apply(&self.transform, original);
        let length = dot(back, back).max(1e-300).sqrt();
        let jacobian = self.det / (length * length * length);
        let cosine = original[2].max(0.0) / std::f64::consts::PI;
        self.magnitude * cosine / jacobian.max(1e-300)
    }

    /// A direction drawn from the fitted distribution: a cosine-weighted
    /// direction put through the transform.
    fn sample(&self, u1: f64, u2: f64) -> [f64; 3] {
        let cos_theta = u1.sqrt();
        let sin_theta = (1.0 - u1).max(0.0).sqrt();
        let phi = std::f64::consts::TAU * u2;
        normalize(apply(
            &self.transform,
            [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta],
        ))
    }
}

/// The lobe's magnitude, its Fresnel term and the direction it points, all from
/// one stratified sweep of its own density.
///
/// The magnitude is `∫ f cos` — the same directional albedo
/// [`crate::dfg::directional_albedo`] holds, which
/// `the_fit_s_magnitude_is_the_dfg_table_s_albedo` is what checks. The average
/// direction is what the fit's frame is built around: a GGX lobe seen at a
/// grazing angle points away from the mirror direction, and a fit that assumed
/// otherwise would spend its three parameters correcting for it.
fn average_terms(view: [f64; 3], alpha: f64) -> (f64, f64, [f64; 3]) {
    let mut magnitude = 0.0f64;
    let mut fresnel = 0.0f64;
    let mut direction = [0.0f64; 3];
    let count = LTC_SAMPLES as usize;
    for j in 0..count {
        for i in 0..count {
            let u1 = (i as f64 + 0.5) / count as f64;
            let u2 = (j as f64 + 0.5) / count as f64;
            let light = ggx_sample(view, alpha, u1, u2);
            let (value, pdf) = ggx_eval(view, light, alpha);
            if pdf <= 0.0 {
                continue;
            }
            let weight = value / pdf;
            let half = normalize([view[0] + light[0], view[1] + light[1], view[2] + light[2]]);
            let grazing = (1.0 - dot(view, half)).max(0.0);
            let grazing2 = grazing * grazing;
            magnitude += weight;
            fresnel += weight * grazing2 * grazing2 * grazing;
            for (slot, value) in direction.iter_mut().zip(light) {
                *slot += weight * value;
            }
        }
    }
    let total = (count * count) as f64;
    // The `y` of the average direction is zero for an isotropic lobe with the
    // view in the `xz` plane, and what the sweep leaves there is quadrature
    // noise. Zeroing it rather than normalising it away keeps the frame in the
    // plane the fit assumes it is in.
    direction[1] = 0.0;
    (magnitude / total, fresnel / total, normalize(direction))
}

/// How far the fitted lobe is from the real one, as the paper measures it.
///
/// The cube of the absolute difference, integrated with multiple importance
/// sampling over both densities. The cube rather than the square is the
/// paper's choice and it matters: it weights the lobe's peak, which is what a
/// highlight's shape is, over the tail, which is what its energy is — and the
/// energy is already exact by construction, because the magnitude comes out of
/// [`average_terms`] rather than out of the fit.
fn fit_error(lobe: &Lobe, view: [f64; 3], alpha: f64) -> f64 {
    let mut error = 0.0f64;
    let count = LTC_SAMPLES as usize;
    for j in 0..count {
        for i in 0..count {
            let u1 = (i as f64 + 0.5) / count as f64;
            let u2 = (j as f64 + 0.5) / count as f64;
            for light in [lobe.sample(u1, u2), ggx_sample(view, alpha, u1, u2)] {
                let (value, pdf_brdf) = ggx_eval(view, light, alpha);
                let fitted = lobe.eval(light);
                let pdf_lobe = if lobe.magnitude > 0.0 {
                    fitted / lobe.magnitude
                } else {
                    0.0
                };
                let denominator = pdf_lobe + pdf_brdf;
                if denominator <= 0.0 {
                    continue;
                }
                let difference = (value - fitted).abs();
                error += difference * difference * difference / denominator;
            }
        }
    }
    error / (count * count) as f64
}

/// Nelder-Mead over the fit's three parameters.
///
/// The downhill simplex method (Nelder and Mead, 1965) in its textbook form:
/// reflect the worst vertex through the centroid of the others, expand if that
/// was an improvement on the best, contract if it was not an improvement on the
/// second worst, and shrink the whole simplex towards the best if the
/// contraction failed too. **Derivative-free**, which is the reason it is here:
/// the objective is a Monte Carlo estimate and has no gradient worth taking.
///
/// Deterministic from `start` — the initial simplex is `start` and `start`
/// displaced by [`FIT_STEP`] along each axis, in that order — so the table is a
/// function of this crate's source and of nothing about the machine that cooks
/// it.
fn nelder_mead(start: [f64; 3], mut objective: impl FnMut([f64; 3]) -> f64) -> [f64; 3] {
    let mut simplex = [start; 4];
    for axis in 0..3 {
        simplex[axis + 1][axis] += FIT_STEP;
    }
    let mut values = simplex.map(&mut objective);

    for _ in 0..LTC_FIT_STEPS {
        // Order by value; four vertices, so an insertion sort is the whole of
        // it and it keeps the pairing with `values` obvious.
        for i in 1..4 {
            let mut j = i;
            while j > 0 && values[j - 1] > values[j] {
                values.swap(j - 1, j);
                simplex.swap(j - 1, j);
                j -= 1;
            }
        }

        let spread = (values[3] - values[0]).abs();
        if spread <= FIT_TOLERANCE * (values[0].abs() + values[3].abs() + FIT_TOLERANCE) {
            break;
        }

        let mut centroid = [0.0f64; 3];
        for vertex in &simplex[..3] {
            for (slot, value) in centroid.iter_mut().zip(vertex) {
                *slot += value / 3.0;
            }
        }
        let along = |factor: f64| {
            let mut point = [0.0f64; 3];
            for (index, slot) in point.iter_mut().enumerate() {
                *slot = centroid[index] + factor * (centroid[index] - simplex[3][index]);
            }
            point
        };

        let reflected = along(1.0);
        let reflected_value = objective(reflected);
        if reflected_value < values[0] {
            let expanded = along(2.0);
            let expanded_value = objective(expanded);
            let (point, value) = if expanded_value < reflected_value {
                (expanded, expanded_value)
            } else {
                (reflected, reflected_value)
            };
            simplex[3] = point;
            values[3] = value;
            continue;
        }
        if reflected_value < values[2] {
            simplex[3] = reflected;
            values[3] = reflected_value;
            continue;
        }
        let contracted = along(-0.5);
        let contracted_value = objective(contracted);
        if contracted_value < values[3] {
            simplex[3] = contracted;
            values[3] = contracted_value;
            continue;
        }
        let best = simplex[0];
        for index in 1..4 {
            for (slot, value) in simplex[index].iter_mut().enumerate() {
                *value = best[slot] + 0.5 * (*value - best[slot]);
            }
            values[index] = objective(simplex[index]);
        }
    }

    let mut best = 0;
    for index in 1..4 {
        if values[index] < values[best] {
            best = index;
        }
    }
    simplex[best]
}

/// The identity, which is both the frame at normal incidence and the transform
/// a fit starts from.
const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// Writes `params` into `lobe`, collapsing them where the slice is isotropic.
///
/// At normal incidence the lobe is rotationally symmetric about the normal, so
/// its shear is zero and its two scales are one scale. Fitting three parameters
/// to a two-parameter problem would leave the extra one wandering on a flat
/// direction of the objective, which is how a Nelder-Mead run ends somewhere a
/// neighbour's warm start cannot be taken from.
fn apply_params(lobe: &mut Lobe, params: [f64; 3], isotropic: bool) {
    let m11 = params[0].max(MIN_SCALE);
    if isotropic {
        lobe.m11 = m11;
        lobe.m22 = m11;
        lobe.m13 = 0.0;
    } else {
        lobe.m11 = m11;
        lobe.m22 = params[1].max(MIN_SCALE);
        lobe.m13 = params[2];
    }
    lobe.update();
}

/// The fitted inverse transform as the four numbers a texel holds.
///
/// Divided through by its middle entry, which the module header argues is free:
/// the polygon integral normalises every transformed vertex, so a positive
/// scale of the whole matrix cannot change its answer. The middle entry is
/// `1 / m22` and therefore always positive, so the division never flips a sign.
fn pack(inverse: &Mat3) -> [f32; 4] {
    let middle = inverse[1][1];
    let scale = 1.0
        / if middle.abs() < 1e-300 {
            1e-300
        } else {
            middle
        };
    [
        (inverse[0][0] * scale) as f32,
        (inverse[0][2] * scale) as f32,
        (inverse[2][0] * scale) as f32,
        (inverse[2][2] * scale) as f32,
    ]
}

/// The fit, run over the whole table.
///
/// The walk is the paper's and its order is load-bearing: **roughest first**,
/// because a rough lobe is nearly the cosine distribution the fit starts from,
/// and **head-on first within a row**, because that slice is isotropic and has
/// one parameter rather than three. Every texel after the first two starts from
/// its neighbour's answer, so no fit has far to walk and none of them needs a
/// global search.
///
/// Each texel is four steps:
///
/// 1. Sweep the lobe's own density for its magnitude, its Fresnel term and the
///    direction it points (`average_terms`).
/// 2. Build the frame around that direction — the paper's `T1`, `T2`, `L` — so
///    the three fitted parameters describe the lobe's *shape* rather than
///    where it points.
/// 3. Minimise `fit_error` over the three parameters with `nelder_mead`,
///    warm-started from the previous texel.
/// 4. Invert, normalise and pack (`pack`).
///
/// In `f64` throughout, for [`crate::dfg::bake`]'s reason.
#[must_use]
pub fn bake() -> Vec<[f32; 4]> {
    let mut table = vec![[0.0f32; 4]; LTC_SIZE * LTC_SIZE];
    // The head-on fit of the previous, rougher row — the only warm start that
    // crosses from one roughness to the next.
    let mut head_on_start = (1.0f64, 1.0f64);

    for roughness_index in (0..LTC_SIZE).rev() {
        let roughness = f64::from(axis_value(roughness_index)).max(LTC_MIN_ROUGHNESS);
        let alpha = roughness * roughness;
        let mut lobe = Lobe {
            magnitude: 1.0,
            m11: head_on_start.0,
            m22: head_on_start.1,
            m13: 0.0,
            frame: IDENTITY,
            transform: IDENTITY,
            inverse: IDENTITY,
            det: 1.0,
        };

        for n_dot_v_index in (0..LTC_SIZE).rev() {
            let n_dot_v = f64::from(axis_value(n_dot_v_index));
            let view = [(1.0 - n_dot_v * n_dot_v).max(0.0).sqrt(), 0.0, n_dot_v];
            let (magnitude, _fresnel, average) = average_terms(view, alpha);
            lobe.magnitude = magnitude;

            let head_on = n_dot_v_index == LTC_SIZE - 1;
            if head_on {
                lobe.frame = IDENTITY;
                lobe.m11 = head_on_start.0;
                lobe.m22 = head_on_start.1;
                lobe.m13 = 0.0;
            } else {
                // `T1`, `T2`, `L` as the paper's own fitter builds them: the
                // lobe's average direction is the frame's `z`, and the other
                // two follow from it because the view stays in the `xz` plane.
                let axis_z = average;
                let axis_x = [axis_z[2], 0.0, -axis_z[0]];
                lobe.frame = [
                    [axis_x[0], 0.0, axis_z[0]],
                    [0.0, 1.0, axis_z[1]],
                    [axis_x[2], 0.0, axis_z[2]],
                ];
            }
            lobe.update();

            let start = [lobe.m11, lobe.m22, lobe.m13];
            let mut trial = lobe;
            let result = nelder_mead(start, |params| {
                apply_params(&mut trial, params, head_on);
                fit_error(&trial, view, alpha)
            });
            apply_params(&mut lobe, result, head_on);

            table[roughness_index * LTC_SIZE + n_dot_v_index] = pack(&lobe.inverse);
            if head_on {
                head_on_start = (lobe.m11, lobe.m22);
            }
        }
    }
    table
}

/// [`bake`]'s output as the bytes `tables/ltc.bin` holds.
#[must_use]
pub fn bake_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(LTC_BYTES);
    for entry in bake() {
        for value in entry {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far a small distant quad's answer may sit from the punctual light it
    /// stands in for, as a share of it, at each `N·V` probed.
    ///
    /// **Three numbers rather than one, because the fit is not uniformly
    /// accurate and pretending otherwise would hide where it is weak.** Seen
    /// head on the fitted lobe is within a few per cent of the real one; at
    /// grazing incidence a GGX lobe grows a long asymmetric tail that a
    /// three-parameter linear transform of a cosine cannot follow, and the
    /// disagreement on the lobe's shoulder is tens of per cent. That is the
    /// paper's own trade — its error norm weights the peak, where a highlight
    /// is — and it is recorded in `docs/backlog.md` with these numbers rather
    /// than absorbed into one loose bound.
    ///
    /// Measured over the probes below: 0.037, 0.156 and 0.394 in the order the
    /// rows appear. Each bound is a little over its measurement.
    const PUNCTUAL_SHARE: [(f32, f32); 3] = [(0.95, 0.06), (0.7, 0.20), (0.4, 0.45)];

    /// How far a texel's half float may sit from the `f32` it was rounded from,
    /// relative to the value.
    ///
    /// Binary16 keeps eleven bits of significand, so a round trip is within
    /// `2⁻¹¹` of the value it started at. This is that, with nothing added.
    const HALF_SHARE: f32 = 1.0 / 2048.0;

    /// The absolute floor under [`HALF_SHARE`]: one step of the subnormal
    /// range, `2⁻²⁴`.
    ///
    /// Below `2⁻¹⁴` a binary16 has fewer than eleven bits, so a relative bound
    /// alone would be a claim the format does not make. The table's shear
    /// column holds entries down to a millionth, where an absolute step of
    /// `2⁻²⁴` is the whole of the accuracy there is — and is also far below
    /// anything a matrix entry beside a one can move.
    const HALF_STEP: f32 = 5.960_464_5e-8;

    /// A quad of half-extent `half` at `distance` along `direction`, facing the
    /// origin, in the order [`polygon_irradiance`] wants.
    ///
    /// The winding is the one `crcbl_render::RectLight` documents: the light's
    /// own `v` axis is `cross(u, emission)`, and the corners run
    /// `-u-v, +u-v, +u+v, -u+v`, which makes the sum positive for a receiver in
    /// front of it.
    fn quad_at(direction: [f32; 3], distance: f32, half: f32) -> ([[f32; 3]; 4], f32) {
        let towards = {
            let length = (direction[0] * direction[0]
                + direction[1] * direction[1]
                + direction[2] * direction[2])
                .sqrt();
            [
                direction[0] / length,
                direction[1] / length,
                direction[2] / length,
            ]
        };
        // The light emits back along the direction it was placed in.
        let emission = [-towards[0], -towards[1], -towards[2]];
        // Any unit vector in the plane; the quad is square, so which one only
        // rotates it about its own normal.
        let seed = if emission[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let u = {
            let along = seed[0] * emission[0] + seed[1] * emission[1] + seed[2] * emission[2];
            let raw = [
                seed[0] - along * emission[0],
                seed[1] - along * emission[1],
                seed[2] - along * emission[2],
            ];
            let length = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
            [raw[0] / length, raw[1] / length, raw[2] / length]
        };
        let v = [
            u[1] * emission[2] - u[2] * emission[1],
            u[2] * emission[0] - u[0] * emission[2],
            u[0] * emission[1] - u[1] * emission[0],
        ];
        let centre = [
            towards[0] * distance,
            towards[1] * distance,
            towards[2] * distance,
        ];
        let corner = |su: f32, sv: f32| {
            [
                centre[0] + su * half * u[0] + sv * half * v[0],
                centre[1] + su * half * u[1] + sv * half * v[1],
                centre[2] + su * half * u[2] + sv * half * v[2],
            ]
        };
        // A square of side `2 half` at `distance`, seen face on.
        let solid_angle = 4.0 * half * half / (distance * distance);
        (
            [
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ],
            solid_angle,
        )
    }

    /// `corners` through the inverse transform at `(n_dot_v, roughness)`.
    fn transformed(corners: [[f32; 3]; 4], n_dot_v: f32, roughness: f32) -> [[f32; 3]; 4] {
        let matrix = inverse_transform(sample(n_dot_v, roughness));
        corners.map(|corner| {
            [
                matrix[0][0] * corner[0] + matrix[0][1] * corner[1] + matrix[0][2] * corner[2],
                matrix[1][0] * corner[0] + matrix[1][1] * corner[1] + matrix[1][2] * corner[2],
                matrix[2][0] * corner[0] + matrix[2][1] * corner[1] + matrix[2][2] * corner[2],
            ]
        })
    }

    /// The engine's own lobe, in the shading frame, as `mesh.slang`'s
    /// `ggx_lobe` writes it — without the `pi` it folded out, so this is the
    /// physical `D · V · F` with `f0` at one.
    fn ggx_lobe(alpha2: f32, n_dot_l: f32, n_dot_v: f32, n_dot_h: f32) -> f32 {
        let shape = n_dot_h * n_dot_h * (alpha2 - 1.0) + 1.0;
        let d = alpha2 / (shape * shape).max(1e-8) / std::f32::consts::PI;
        let lambda_v = n_dot_l * (n_dot_v * n_dot_v * (1.0 - alpha2) + alpha2).sqrt();
        let lambda_l = n_dot_v * (n_dot_l * n_dot_l * (1.0 - alpha2) + alpha2).sqrt();
        let visibility = 0.5 / (lambda_v + lambda_l).max(1e-6);
        d * visibility
    }

    #[test]
    fn every_entry_is_finite_and_the_table_is_the_size_it_claims() {
        assert_eq!(bytes().len(), LTC_BYTES);
        let mut widest = 0.0f32;
        for roughness in 0..LTC_SIZE {
            for n_dot_v in 0..LTC_SIZE {
                for value in entry(n_dot_v, roughness) {
                    assert!(
                        value.is_finite(),
                        "({n_dot_v}, {roughness}) holds {value}, which is not a number the shader \
                         can multiply by"
                    );
                    widest = widest.max(value.abs());
                }
            }
        }
        // Half floats stop at 65504, and an entry past that would reach the
        // image as an infinity — the one way this encoding can fail silently.
        assert!(
            widest < 65_504.0,
            "the widest entry is {widest}, which `Rgba16Float` cannot hold"
        );
    }

    #[test]
    fn the_polygon_integral_is_invariant_to_the_transform_s_scale() {
        // **The property the four-number packing rests on.** The fit produces
        // five numbers and the table stores four, because the integral cannot
        // see a positive scale of the matrix — every transformed vertex is
        // normalised before it is integrated. Asserting that `inverse_transform`
        // puts a one in the middle would assert nothing, since it writes the
        // literal; this asserts the reason the literal is allowed to be there.
        let (corners, _) = quad_at([0.3, 0.2, 1.0], 2.0, 0.7);
        let matrix = inverse_transform(entry(40, 40));
        let scaled = matrix.map(|row| row.map(|value| value * 7.5));
        let apply = |m: [[f32; 3]; 3]| {
            polygon_irradiance(corners.map(|corner| {
                [
                    m[0][0] * corner[0] + m[0][1] * corner[1] + m[0][2] * corner[2],
                    m[1][0] * corner[0] + m[1][1] * corner[1] + m[1][2] * corner[2],
                    m[2][0] * corner[0] + m[2][1] * corner[1] + m[2][2] * corner[2],
                ]
            }))
        };
        let plain = apply(matrix);
        let stretched = apply(scaled);
        // Anti-vacuity: a transform that gathered nothing would agree with
        // itself scaled, and say nothing about the normalisation.
        assert!(plain > 0.05, "the probe gathers only {plain}");
        assert!(
            (plain - stretched).abs() < 1e-4,
            "the same polygon gathers {plain} under the fitted transform and {stretched} \
             under the same transform scaled, so the four-number packing is losing the fifth"
        );
    }

    #[test]
    fn normal_incidence_has_no_shear() {
        // At normal incidence the slice through the lobe is rotationally
        // symmetric about the normal, so the transform is a scale and the two
        // off-diagonal entries are zero exactly. Every other column has a shear,
        // which is what makes this a claim rather than a restatement of the
        // packing.
        for roughness in 0..LTC_SIZE {
            let [_, b, c, _] = entry(LTC_SIZE - 1, roughness);
            assert_eq!(
                (b, c),
                (0.0, 0.0),
                "the head-on column at roughness {roughness} is sheared"
            );
        }
        let sheared = (0..LTC_SIZE)
            .filter(|&n_dot_v| entry(n_dot_v, 40)[1].abs() > 1e-3)
            .count();
        assert!(
            sheared > LTC_SIZE / 2,
            "only {sheared} columns carry a shear, so the head-on zero above says nothing"
        );
    }

    #[test]
    fn a_polygon_covering_the_hemisphere_gathers_pi() {
        // A quad enormously wider than its distance covers everything above the
        // horizon, and the clamped-cosine integral over a whole hemisphere is
        // `pi`. This is the one value the integral has that nothing about the
        // fit or the packing can move.
        let (corners, _) = quad_at([0.0, 0.0, 1.0], 1.0, 1.0e5);
        let gathered = polygon_irradiance(corners);
        assert!(
            (gathered - std::f32::consts::PI).abs() < 1e-3,
            "a hemisphere gathers {gathered}, not pi"
        );
    }

    #[test]
    fn a_polygon_below_the_horizon_gathers_nothing() {
        let (corners, _) = quad_at([0.0, 0.0, -1.0], 1.0, 0.5);
        assert_eq!(polygon_irradiance(corners), 0.0);
        // And one behind the light, which is the winding rather than the
        // horizon: the same quad wound the other way round.
        let (mut wound, _) = quad_at([0.0, 0.0, 1.0], 1.0, 0.5);
        wound.swap(1, 3);
        assert_eq!(polygon_irradiance(wound), 0.0);
    }

    /// The clamped-cosine integral over a quad, by brute force: sweep the
    /// hemisphere on a fine grid and add `cos θ` wherever the direction hits
    /// the quad.
    ///
    /// The independent reference [`polygon_irradiance`]'s closed form is
    /// checked against — `crate::dfg`'s `the_table_agrees_with_a_second_integration`
    /// is the same arrangement. It knows nothing about spherical polygons, the
    /// edge fit or the horizon clip: it fires rays and asks whether they land
    /// on a rectangle, so every one of those three has to be right for the two
    /// to meet.
    fn quadrature(centre: [f32; 3], u: [f32; 3], v: [f32; 3], steps: usize) -> f32 {
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let u_length2 = dot(u, u);
        let v_length2 = dot(v, v);
        let plane = dot(centre, normal);

        let mut total = 0.0f64;
        for theta_step in 0..steps {
            // Midpoints in both angles, so the sum is a plain midpoint rule.
            let theta = std::f32::consts::FRAC_PI_2 * (theta_step as f32 + 0.5) / steps as f32;
            let (sin_theta, cos_theta) = theta.sin_cos();
            for phi_step in 0..2 * steps {
                let phi = std::f32::consts::TAU * (phi_step as f32 + 0.5) / (2 * steps) as f32;
                let (sin_phi, cos_phi) = phi.sin_cos();
                let ray = [sin_theta * cos_phi, sin_theta * sin_phi, cos_theta];
                let along = dot(ray, normal);
                if along.abs() < 1e-9 {
                    continue;
                }
                let distance = plane / along;
                if distance <= 0.0 {
                    continue;
                }
                let hit = [
                    distance * ray[0] - centre[0],
                    distance * ray[1] - centre[1],
                    distance * ray[2] - centre[2],
                ];
                if dot(hit, u).abs() <= u_length2 && dot(hit, v).abs() <= v_length2 {
                    total += f64::from(cos_theta * sin_theta);
                }
            }
        }
        // `dθ dφ` for the grid above.
        let cell = f64::from(std::f32::consts::FRAC_PI_2) / steps as f64
            * f64::from(std::f32::consts::TAU)
            / (2 * steps) as f64;
        (total * cell) as f32
    }

    #[test]
    fn the_polygon_integral_agrees_with_a_hemisphere_quadrature() {
        // Four quads, chosen so the clipper has to work: one wholly above the
        // horizon, one leaning through it, one standing on it, and one so wide
        // it wraps most of the sky.
        let probes: [([f32; 3], [f32; 3], [f32; 3]); 4] = [
            ([0.0, 0.0, 2.0], [0.8, 0.0, 0.0], [0.0, 0.6, 0.0]),
            ([1.0, 0.2, 0.9], [0.7, 0.0, -0.9], [0.0, 1.1, 0.0]),
            ([1.4, 0.0, 0.0], [0.0, 0.0, 1.2], [0.0, 1.3, 0.0]),
            ([0.0, 0.0, 1.0], [2.6, 0.0, 0.9], [0.0, 2.4, 0.0]),
        ];
        let mut largest = 0.0f32;
        for (centre, u, v) in probes {
            let corner = |su: f32, sv: f32| {
                [
                    centre[0] + su * u[0] + sv * v[0],
                    centre[1] + su * u[1] + sv * v[1],
                    centre[2] + su * u[2] + sv * v[2],
                ]
            };
            let corners = [
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ];
            // Whichever winding faces the receiver; the quadrature is
            // two-sided and this integral is not, which is the one thing the
            // two do not share.
            let closed = polygon_irradiance(corners).max(polygon_irradiance([
                corners[0], corners[3], corners[2], corners[1],
            ]));
            let swept = quadrature(centre, u, v, 900);
            let gap = (closed - swept).abs();
            assert!(
                gap < 4.0e-3,
                "the closed form gathers {closed} at {centre:?} where the sweep gathers {swept}"
            );
            largest = largest.max(closed);
        }
        // Anti-vacuity: at least one probe gathers a real amount of light, so
        // four zeroes could not have passed this.
        assert!(
            largest > 1.0,
            "the brightest probe gathers only {largest}, so the agreement is between two zeroes"
        );
    }

    #[test]
    fn a_small_distant_quad_gathers_what_a_punctual_light_would() {
        // The clamped cosine's own limit: a quad small against its distance
        // subtends `solid_angle` and gathers `solid_angle * cos`.
        for direction in [
            [0.0, 0.0, 1.0],
            [0.5, 0.0, 1.0],
            [0.0, 0.6, 1.0],
            [-0.7, 0.3, 1.0],
        ] {
            let (corners, solid_angle) = quad_at(direction, 40.0, 0.25);
            let length = (direction[0] * direction[0]
                + direction[1] * direction[1]
                + direction[2] * direction[2])
                .sqrt();
            let punctual = solid_angle * direction[2] / length;
            let gathered = polygon_irradiance(corners);
            let share = (gathered - punctual).abs() / punctual;
            assert!(
                share < 1e-3,
                "a quad at {direction:?} gathers {gathered} where a punctual light gathers \
                 {punctual}, a share of {share}"
            );
        }
    }

    #[test]
    fn the_fitted_lobe_answers_what_the_ggx_lobe_answers_for_a_small_quad() {
        // **The test the whole table exists to pass.** A quad small enough to be
        // a point must produce the specular response `mesh.slang` already
        // produces for a point light in that direction — so a transposed
        // matrix, a mis-ordered pack or a frame built the wrong way round has
        // nowhere to hide, and neither does an `f0` scale taken from the wrong
        // table.
        let mut largest = 0.0f32;
        for roughness in [0.2f32, 0.35, 0.5, 0.7, 0.9] {
            for (n_dot_v, bound) in PUNCTUAL_SHARE {
                let view = [(1.0 - n_dot_v * n_dot_v).sqrt(), 0.0, n_dot_v];
                // Probed **around the mirror direction**, which is where a
                // highlight is and what the fit's error norm weights. A probe
                // out in the tail of a sharp lobe would be a claim about the
                // paper's choice of norm rather than about this table.
                let mirror = [-view[0], 0.0, view[2]];
                let towards = |x: f32, y: f32| [mirror[0] + x, y, mirror[2]];
                for direction in [
                    towards(0.0, 0.0),
                    towards(0.25, 0.0),
                    towards(-0.25, 0.0),
                    towards(0.0, 0.3),
                ] {
                    let (corners, solid_angle) = quad_at(direction, 60.0, 0.35);
                    let length = (direction[0] * direction[0]
                        + direction[1] * direction[1]
                        + direction[2] * direction[2])
                        .sqrt();
                    let to_light = [
                        direction[0] / length,
                        direction[1] / length,
                        direction[2] / length,
                    ];
                    let half = {
                        let raw = [
                            to_light[0] + view[0],
                            to_light[1] + view[1],
                            to_light[2] + view[2],
                        ];
                        let l = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
                        [raw[0] / l, raw[1] / l, raw[2] / l]
                    };
                    let alpha = roughness * roughness;
                    // `f0` at one: Schlick is then one everywhere and the
                    // table's scale plus bias is the whole of the lobe's
                    // energy, so the two sides compare the same quantity.
                    let punctual = ggx_lobe(alpha * alpha, to_light[2], n_dot_v, half[2])
                        * to_light[2]
                        * solid_angle;

                    let scale = crate::dfg::directional_albedo(n_dot_v, roughness);
                    let area = polygon_irradiance(transformed(corners, n_dot_v, roughness)) * scale
                        / std::f32::consts::PI;

                    let share = (area - punctual).abs() / punctual;
                    largest = largest.max(share);
                    assert!(
                        share < bound,
                        "at roughness {roughness}, N·V {n_dot_v}, towards {direction:?}: the fit \
                         answers {area} where the lobe answers {punctual}, a share of {share}"
                    );
                }
            }
        }
        // Anti-vacuity: the probes above must actually exercise the fit rather
        // than all land on a value the identity transform would give too.
        assert!(
            largest > 1e-3,
            "every probe agreed to {largest}, which is too good — the two sides are probably the \
             same arithmetic"
        );
    }

    #[test]
    fn the_fit_s_magnitude_is_the_dfg_table_s_albedo() {
        // The two tables integrate the same lobe: `crate::dfg`'s `scale + bias`
        // is `∫ f cos` with Schlick split in two and put back together, and the
        // magnitude the fit scales its distribution by is that same integral
        // drawn from the same density. They are cooked by different code in
        // different modules, so agreement is evidence and disagreement would
        // mean one of them describes a lobe this engine does not shade with.
        let mut largest = 0.0f64;
        for (n_dot_v_index, roughness_index) in [(63usize, 63usize), (48, 40), (32, 32), (12, 20)] {
            let n_dot_v = f64::from(axis_value(n_dot_v_index));
            let roughness = f64::from(axis_value(roughness_index)).max(LTC_MIN_ROUGHNESS);
            let view = [(1.0 - n_dot_v * n_dot_v).max(0.0).sqrt(), 0.0, n_dot_v];
            let (magnitude, _, _) = average_terms(view, roughness * roughness);
            let table = f64::from(crate::dfg::directional_albedo(
                n_dot_v as f32,
                roughness as f32,
            ));
            let share = (magnitude - table).abs() / table;
            largest = largest.max(share);
            assert!(
                share < 0.02,
                "at ({n_dot_v_index}, {roughness_index}) the fit measures {magnitude} where the \
                 DFG table holds {table}"
            );
        }
        assert!(
            largest > 0.0,
            "the two integrals agreed exactly, which means one of them is calling the other"
        );
    }

    #[test]
    fn a_half_round_trips_every_value_the_table_holds() {
        let mut largest = 0.0f32;
        for roughness in 0..LTC_SIZE {
            for n_dot_v in 0..LTC_SIZE {
                for value in entry(n_dot_v, roughness) {
                    let back = half_value(half_bits(value));
                    let bound = (HALF_SHARE * value.abs()).max(HALF_STEP);
                    let gap = (back - value).abs();
                    assert!(
                        gap <= bound,
                        "{value} round trips to {back}, which is {gap} away and past {bound}"
                    );
                    largest = largest.max(gap / bound);
                }
            }
        }
        // Anti-vacuity: the bound has to be tight enough that something
        // approaches it, or it is not measuring the encoder at all.
        assert!(
            largest > 0.3,
            "the worst round trip used {largest} of its bound, so the bound is not a bound"
        );
    }

    #[test]
    fn a_half_carries_the_edges_of_its_own_range() {
        for (value, expected) in [
            (0.0f32, 0x0000u16),
            (-0.0, 0x8000),
            (1.0, 0x3c00),
            (-2.0, 0xc000),
            (65504.0, 0x7bff),
            // Past the largest finite half, so it saturates rather than wrapping.
            (1.0e6, 0x7c00),
            // The smallest normal, and a subnormal under it.
            (6.103_515_6e-5, 0x0400),
            (5.960_464_5e-8, 0x0001),
            // Under half the smallest subnormal, so it rounds to zero.
            (1.0e-9, 0x0000),
        ] {
            assert_eq!(
                half_bits(value),
                expected,
                "{value} encodes to {:#06x}, not {expected:#06x}",
                half_bits(value)
            );
        }
        assert!(half_value(half_bits(f32::INFINITY)).is_infinite());
        assert!(half_value(half_bits(f32::NAN)).is_nan());
    }

    #[test]
    fn the_texels_are_the_committed_table_in_half_floats() {
        let texels = texels();
        assert_eq!(texels.len(), LTC_TEXELS_BYTES);
        let mut differing = 0usize;
        for roughness in 0..LTC_SIZE {
            for n_dot_v in 0..LTC_SIZE {
                let at = (roughness * LTC_SIZE + n_dot_v) * LTC_TEXEL_BYTES;
                for (channel, value) in entry(n_dot_v, roughness).into_iter().enumerate() {
                    let word = at + channel * 2;
                    let bits = u16::from_le_bytes([texels[word], texels[word + 1]]);
                    let back = half_value(bits);
                    let bound = (HALF_SHARE * value.abs()).max(HALF_STEP);
                    assert!(
                        (back - value).abs() <= bound,
                        "texel ({n_dot_v}, {roughness}) channel {channel} holds {back} where the \
                         table holds {value}"
                    );
                    if back != value {
                        differing += 1;
                    }
                }
            }
        }
        // The rounding has to actually round something, or this test would pass
        // over an encoder that copied `f32` bytes into the wrong-sized image.
        assert!(
            differing > LTC_SIZE * LTC_SIZE,
            "only {differing} channels changed under the half rounding"
        );
    }

    #[test]
    fn a_sample_at_a_texel_centre_is_that_texel() {
        for (n_dot_v, roughness) in [(0usize, 0usize), (17, 40), (63, 63), (5, 61)] {
            let at = sample(axis_value(n_dot_v), axis_value(roughness));
            assert_eq!(at, entry(n_dot_v, roughness));
        }
    }

    #[test]
    fn sampling_past_the_edges_clamps_to_them() {
        assert_eq!(sample(0.0, 0.0), entry(0, 0));
        assert_eq!(sample(1.0, 1.0), entry(LTC_SIZE - 1, LTC_SIZE - 1));
        assert_eq!(sample(-5.0, 5.0), entry(0, LTC_SIZE - 1));
    }
}
