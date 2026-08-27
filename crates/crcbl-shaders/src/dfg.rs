//! The split-sum `DFG` table: how much of the light a GGX lobe hands back.
//!
//! One 2D table over `(N·V, roughness)`, two channels, baked once and committed
//! to `tables/dfg.bin`. Its entry is a **scale and a bias on `f0`**, so the
//! fraction of arriving light a surface's specular lobe returns is
//!
//! ```text
//! E(f0) = f0 * scale + bias
//! ```
//!
//! which is Karis's split-sum factorisation of the specular integral. Two
//! separate features read it and [`energy_compensation`] is the first of them:
//!
//! * **Multi-scatter energy compensation.** A single-scatter GGX lobe accounts
//!   for light that leaves the microsurface after one bounce and drops
//!   everything that bounces again, so it returns less than it received and the
//!   shortfall grows with roughness. [`directional_albedo`] is exactly how much
//!   comes back, and [`energy_compensation`] is the factor that puts the rest
//!   in. `docs/plan/44-lighting.md`'s rung 1.
//! * **Specular image-based lighting**, whose second split-sum half is this
//!   same table — rung 3 there. It is one table because it is one integral.
//!
//! # Why the bytes are committed rather than computed
//!
//! [`bake`] is the integrator and it importance-samples the lobe, which means
//! `sin`, `cos` and a `powf` — evaluated on a machine whose `libm` is not the
//! one CI's macOS and Windows runners have, for goldens that are blessed once
//! and compared on all four backends. A table computed at build time would
//! therefore be four slightly different tables, and every reflective pixel in
//! every golden would disagree by a last place somewhere.
//!
//! So the table is data: baked on one machine, committed, and read by everyone.
//! That is the same arrangement `clusters/dunes.dag` is under and for a related
//! reason, and it is one of the two escapes this crate has from its own
//! no-transcendentals rule — a `pow` evaluated once at cook time and stored is
//! not a `pow` four platforms evaluate per fragment. The other is
//! [`crate::fog`]: build the function out of the operations the rule permits,
//! which needs no artifact but only works where such a construction exists.
//!
//! Regenerate or verify with the tool that owns it:
//!
//! ```text
//! cargo run -p crcbl-shaders --example cook-dfg            # regenerate
//! cargo run -p crcbl-shaders --example cook-dfg -- --check # verify only
//! ```

/// Texels along each axis of the table.
///
/// Square, and both axes are sampled at texel centres — see [`axis_value`]. 64
/// is what Karis's original presentation used and what Filament ships; the
/// function it stores is smooth in both arguments, so the error a finer table
/// would remove is far below the quantisation of the `Rgba8Unorm` targets this
/// engine's reflections land in.
pub const DFG_SIZE: usize = 64;

/// Bytes one entry occupies: two little-endian `f32`s, scale then bias.
///
/// `f32` rather than the `f16` pair a GPU would sample: the whole table is
/// [`DFG_BYTES`] either way, which is nothing, and half-precision would put a
/// rounding step between the integrator and the committed bytes that this
/// crate cannot perform without a dependency.
pub const DFG_ENTRY_BYTES: usize = 8;

/// The committed table's exact length.
///
/// The artifact is typed `&[u8; DFG_BYTES]` where it is included, so a table of
/// the wrong size fails to compile rather than being caught by a test.
pub const DFG_BYTES: usize = DFG_SIZE * DFG_SIZE * DFG_ENTRY_BYTES;

/// How many samples [`bake`] draws per texel.
///
/// A power of two because the radical inverse below is base 2, so the sequence
/// covers `[0, 1)` at exactly this stratification with no partial stratum. The
/// residual Monte Carlo noise at this count is measured by
/// `the_table_is_smooth_along_both_of_its_axes` rather than assumed.
pub const DFG_SAMPLES: u32 = 1024;

/// The committed table, `tables/dfg.bin`.
///
/// Not a path anything resolves at run time: it is in the binary, exactly as
/// the compiled shaders are, so there is no file for a deployment to lose.
const TABLE: &[u8; DFG_BYTES] = include_bytes!("../tables/dfg.bin");

/// Where along an axis texel `index` sits, at the texel's centre.
///
/// Centres rather than edges, which is what a GPU's `SampleLevel` with a linear
/// filter and clamped addressing reads: texel 0 stands for `0.5 / DFG_SIZE`
/// rather than for zero. A table baked on edges and sampled on centres is off by
/// half a texel everywhere, which is a smooth error and therefore one nothing
/// catches by looking.
#[must_use]
pub fn axis_value(index: usize) -> f32 {
    (index as f32 + 0.5) / DFG_SIZE as f32
}

/// The committed entry at `(n_dot_v_index, roughness_index)`, as `(scale, bias)`.
///
/// # Panics
///
/// If either index is at or past [`DFG_SIZE`].
#[must_use]
pub fn entry(n_dot_v_index: usize, roughness_index: usize) -> [f32; 2] {
    assert!(
        n_dot_v_index < DFG_SIZE && roughness_index < DFG_SIZE,
        "({n_dot_v_index}, {roughness_index}) is outside a {DFG_SIZE}-square table"
    );
    let at = (roughness_index * DFG_SIZE + n_dot_v_index) * DFG_ENTRY_BYTES;
    let scale = f32::from_le_bytes([TABLE[at], TABLE[at + 1], TABLE[at + 2], TABLE[at + 3]]);
    let bias = f32::from_le_bytes([TABLE[at + 4], TABLE[at + 5], TABLE[at + 6], TABLE[at + 7]]);
    [scale, bias]
}

/// The committed table's bytes, for a caller uploading it to a device.
///
/// Row-major, `roughness` slow and `N·V` fast, which is the order a 2D image
/// upload expects and the order [`entry`] indexes in.
#[must_use]
pub const fn bytes() -> &'static [u8; DFG_BYTES] {
    TABLE
}

/// Bytes one texel of [`albedo_texels`] occupies: an `Rg8Unorm` pair holding
/// one 16-bit fixed-point number, high byte in red and low byte in green.
///
/// **A split byte pair rather than a half float**, which is the format
/// `crcbl_hal::Format::Rg16Float`'s own doc comment anticipates for a BRDF
/// table — not a link, because this crate does not depend on that one. Two
/// reasons, and neither is style. The value stored is a *share of
/// arriving light*, so it lives in `[0, 1]`, where a fixed-point step of
/// `1 / 65535` is finer everywhere than half precision's — which near one is
/// `2^-11`, thirty times coarser. And the conversion is an integer split rather
/// than IEEE 754 binary16 rounding, which this crate would otherwise have to
/// transcribe: [`DFG_ENTRY_BYTES`] says in as many words that it cannot perform
/// that rounding without a dependency.
pub const ALBEDO_TEXEL_BYTES: usize = 2;

/// The length of [`albedo_texels`], one [`ALBEDO_TEXEL_BYTES`] per texel of a
/// [`DFG_SIZE`]-square image.
pub const ALBEDO_BYTES: usize = DFG_SIZE * DFG_SIZE * ALBEDO_TEXEL_BYTES;

/// The committed table reduced to the one number a specular lobe's energy
/// compensation needs, encoded for upload as an `Rg8Unorm` image.
///
/// [`directional_albedo`] at every texel centre — `scale + bias`, the table at
/// `f0 = 1` — rather than the two channels the entry holds. Compensation is the
/// only reader today and it wants the sum; the second split-sum consumer,
/// image-based specular lighting, wants the pair and will upload its own image
/// from the same [`bytes`], so nothing is lost by keeping this one at what its
/// caller reads.
///
/// Row-major with `roughness` slow and `N·V` fast, which is [`entry`]'s order
/// and the order a 2D image upload expects.
#[must_use]
pub fn albedo_texels() -> Vec<u8> {
    let mut texels = Vec::with_capacity(ALBEDO_BYTES);
    for roughness in 0..DFG_SIZE {
        for n_dot_v in 0..DFG_SIZE {
            let [scale, bias] = entry(n_dot_v, roughness);
            // Clamped before quantising, not after: the table is a Monte Carlo
            // estimate and its smoothest row lands a few parts in a hundred
            // thousand above one, which would wrap the high byte to zero.
            let quantised = ((scale + bias).clamp(0.0, 1.0) * 65535.0).round() as u32;
            texels.push((quantised >> 8) as u8);
            texels.push((quantised & 0xff) as u8);
        }
    }
    texels
}

/// One texel of [`albedo_texels`] as the value a shader reads back out of it.
///
/// `red` and `green` are what a sampled `Rg8Unorm` hands a fragment: the stored
/// bytes over 255, a conversion every API defines exactly. Multiplying each
/// back by 255 recovers the two bytes, and `high * 256 + low` over 65535 is the
/// fixed-point number [`albedo_texels`] wrote.
///
/// **This is the shader's arithmetic, transcribed**, so that
/// `the_texels_decode_to_the_table_they_were_baked_from` is a test of what the
/// GPU will compute rather than of a second encoding that happens to agree.
/// `shaders/mesh.slang`'s `decode_specular_albedo` is the other copy.
#[must_use]
pub fn decode_albedo(red: f32, green: f32) -> f32 {
    (red * 65280.0 + green * 255.0) / 65535.0
}

/// The table sampled bilinearly, the way a shader's linear filter reads it.
///
/// Clamped at both edges rather than wrapped, again matching the sampler: a
/// `N·V` of zero is a surface exactly edge-on and reads the first texel's value
/// rather than the last row's.
#[must_use]
pub fn sample(n_dot_v: f32, roughness: f32) -> [f32; 2] {
    let axis = |value: f32| {
        let scaled = value.clamp(0.0, 1.0) * DFG_SIZE as f32 - 0.5;
        let low = scaled.floor().clamp(0.0, (DFG_SIZE - 1) as f32);
        let high = (low + 1.0).min((DFG_SIZE - 1) as f32);
        (low as usize, high as usize, (scaled - low).clamp(0.0, 1.0))
    };
    let (x0, x1, fx) = axis(n_dot_v);
    let (y0, y1, fy) = axis(roughness);
    let mut out = [0.0f32; 2];
    for (channel, slot) in out.iter_mut().enumerate() {
        let top = entry(x0, y0)[channel] * (1.0 - fx) + entry(x1, y0)[channel] * fx;
        let bottom = entry(x0, y1)[channel] * (1.0 - fx) + entry(x1, y1)[channel] * fx;
        *slot = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// What fraction of arriving light the single-scatter lobe hands back, for a
/// surface that reflects everything it does not lose.
///
/// The table at `f0 = 1`, which is `scale + bias`. This is the furnace test's
/// answer: a white surface under uniform white light must come back white, and
/// the amount by which this falls short of one is exactly the energy the
/// single-scatter model dropped.
#[must_use]
pub fn directional_albedo(n_dot_v: f32, roughness: f32) -> f32 {
    let [scale, bias] = sample(n_dot_v, roughness);
    scale + bias
}

/// [`directional_albedo`] as the shader computes it: bilinear over
/// [`albedo_texels`]' quantised image rather than over the committed `f32`s.
///
/// The same four-tap filter [`sample`] performs, on decoded texels, with the
/// same clamped addressing. It exists so the quantisation has a number attached
/// — `the_texels_decode_to_the_table_they_were_baked_from` measures this
/// against [`directional_albedo`] — and so a caller that must agree with the
/// GPU exactly has something to agree with.
#[must_use]
pub fn sampled_albedo(n_dot_v: f32, roughness: f32) -> f32 {
    let axis = |value: f32| {
        let scaled = value.clamp(0.0, 1.0) * DFG_SIZE as f32 - 0.5;
        let low = scaled.floor().clamp(0.0, (DFG_SIZE - 1) as f32);
        let high = (low + 1.0).min((DFG_SIZE - 1) as f32);
        (low as usize, high as usize, (scaled - low).clamp(0.0, 1.0))
    };
    let texels = albedo_texels();
    let at = |x: usize, y: usize| {
        let index = (y * DFG_SIZE + x) * ALBEDO_TEXEL_BYTES;
        decode_albedo(
            f32::from(texels[index]) / 255.0,
            f32::from(texels[index + 1]) / 255.0,
        )
    };
    let (x0, x1, fx) = axis(n_dot_v);
    let (y0, y1, fy) = axis(roughness);
    let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
    let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// The factor the specular lobe is multiplied by to put the multiply-scattered
/// energy back.
///
/// `1 + f0 * (1 / E - 1)`, per channel, where `E` is [`directional_albedo`].
/// The derivation is one line: the lobe returns `E`, the missing `1 - E` left
/// the surface after further bounces and each bounce tints it by the surface's
/// reflectance, so the total wanted is `E + f0 (1 - E)` and this is that over
/// `E`. At `f0 = 1` it is `1 / E` and the furnace comes back white exactly,
/// which `the_compensation_closes_the_furnace` is what checks.
///
/// **Never below one, by construction rather than by luck.** The albedo is
/// clamped to at most one before it is inverted, because the table is a Monte
/// Carlo estimate and its smoothest row lands a few parts in a hundred thousand
/// *above* one — which without the clamp is a negative gain, and a mirror
/// dimmed by a compensation term that exists to brighten things. Clamped here,
/// so every caller gets the guarantee instead of each one remembering it; the
/// shader that samples this table owes the same clamp for the same reason.
///
/// A smooth surface therefore has `E` at one and the factor at exactly one, and
/// the polished half of every material set is untouched.
#[must_use]
pub fn energy_compensation(f0: [f32; 3], n_dot_v: f32, roughness: f32) -> [f32; 3] {
    let albedo = directional_albedo(n_dot_v, roughness).clamp(1e-4, 1.0);
    let gain = 1.0 / albedo - 1.0;
    [1.0 + f0[0] * gain, 1.0 + f0[1] * gain, 1.0 + f0[2] * gain]
}

/// The `i`th point of the base-2 Hammersley sequence over `count` points.
///
/// `(i / count, radical_inverse_2(i))` — the second coordinate is `i`'s bits
/// reversed and read as a fraction, which is `u32::reverse_bits` and therefore
/// exact on every machine. The sequence is what stratifies [`bake`]'s samples;
/// its determinism is not what the committed table rests on, but it is why two
/// bakes on one machine agree to the last bit.
fn hammersley(i: u32, count: u32) -> (f64, f64) {
    // `2^-32` exactly, so the reversed bits land in `[0, 1)` with no rounding.
    let radical = f64::from(i.reverse_bits()) * (1.0 / 4_294_967_296.0);
    (f64::from(i) / f64::from(count), radical)
}

/// Integrate the split-sum table from the same lobe `shaders/mesh.slang` shades
/// with, and return it row-major with `roughness` slow.
///
/// The estimator, for a view direction at `n_dot_v` from the normal:
///
/// 1. Importance-sample a half-vector from the Trowbridge-Reitz distribution,
///    whose `cos θ` is `sqrt((1 - ξ) / (1 + (α² - 1) ξ))`.
/// 2. Reflect the view about it to get the light direction, and skip the sample
///    if that lands below the surface.
/// 3. Weight by `4 V N·L V·H / N·H`, which is the BRDF over the sampling
///    density with the distribution cancelled — so the table never evaluates `D`
///    at all, and the `α² → 0` singularity a mirror has cannot reach it.
/// 4. Split Schlick's Fresnel into the part that scales `f0` and the part that
///    does not, and accumulate the two separately. That split is the whole
///    trick: it is what turns one integral per material into one table.
///
/// `V` is the Smith height-correlated visibility, spelled exactly as
/// `ggx_lobe` spells it — the table has to describe the lobe this engine
/// actually shades with, not the lobe the reference implementations use. In
/// `f64` throughout, because the sum runs over [`DFG_SAMPLES`] terms and the
/// result is rounded to `f32` once at the end rather than [`DFG_SAMPLES`] times.
#[must_use]
pub fn bake() -> Vec<[f32; 2]> {
    let mut table = Vec::with_capacity(DFG_SIZE * DFG_SIZE);
    for roughness_index in 0..DFG_SIZE {
        let roughness = f64::from(axis_value(roughness_index));
        let alpha = roughness * roughness;
        let alpha2 = alpha * alpha;
        for n_dot_v_index in 0..DFG_SIZE {
            let n_dot_v = f64::from(axis_value(n_dot_v_index));
            // The view in the normal's frame, with the normal at `+Z`. Only the
            // angle between them matters, so the azimuth is free and this takes
            // zero.
            let view = [(1.0 - n_dot_v * n_dot_v).max(0.0).sqrt(), 0.0, n_dot_v];
            let mut scale = 0.0f64;
            let mut bias = 0.0f64;
            for i in 0..DFG_SAMPLES {
                let (u, v) = hammersley(i, DFG_SAMPLES);
                let phi = std::f64::consts::TAU * u;
                let cos_theta = ((1.0 - v) / (1.0 + (alpha2 - 1.0) * v)).max(0.0).sqrt();
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                let half = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                let v_dot_h = view[0] * half[0] + view[1] * half[1] + view[2] * half[2];
                let light = [
                    2.0 * v_dot_h * half[0] - view[0],
                    2.0 * v_dot_h * half[1] - view[1],
                    2.0 * v_dot_h * half[2] - view[2],
                ];
                let n_dot_l = light[2];
                if n_dot_l <= 0.0 || v_dot_h <= 0.0 {
                    continue;
                }
                let n_dot_h = half[2].max(1e-8);
                let lambda_v = n_dot_l * (n_dot_v * n_dot_v * (1.0 - alpha2) + alpha2).sqrt();
                let lambda_l = n_dot_v * (n_dot_l * n_dot_l * (1.0 - alpha2) + alpha2).sqrt();
                let visibility = 0.5 / (lambda_v + lambda_l).max(1e-12);
                let weight = 4.0 * visibility * n_dot_l * v_dot_h / n_dot_h;
                // Schlick split in two: `f0` carries `1 - grazing^5` and the
                // white tail carries `grazing^5`. As repeated multiplication,
                // for the reason `ggx_lobe` writes it that way.
                let grazing = 1.0 - v_dot_h;
                let grazing2 = grazing * grazing;
                let tail = grazing2 * grazing2 * grazing;
                scale += (1.0 - tail) * weight;
                bias += tail * weight;
            }
            let count = f64::from(DFG_SAMPLES);
            table.push([(scale / count) as f32, (bias / count) as f32]);
        }
    }
    table
}

/// [`bake`]'s output as the bytes `tables/dfg.bin` holds.
#[must_use]
pub fn bake_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DFG_BYTES);
    for [scale, bias] in bake() {
        bytes.extend_from_slice(&scale.to_le_bytes());
        bytes.extend_from_slice(&bias.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far a value may sit outside `[0, 1]` before it stops being a share of
    /// the light that arrived.
    ///
    /// The table is a Monte Carlo estimate, so a texel whose true value is one
    /// lands a little either side of it. Measured over the committed table: the
    /// largest excess above one is `4.6e-5`, at the smoothest row where the
    /// answer is exactly one. Twice that, so the bound is a bound rather than
    /// the measurement.
    const NOISE: f32 = 1e-4;

    /// The first `N·V` column the directional albedo falls monotonically in.
    ///
    /// Below this the table is genuinely not monotone and that is the lobe
    /// rather than the estimator: at `N·V` under about a fifth the albedo dips
    /// and recovers — column zero runs `1.000, 0.997, 0.976, 0.931, 0.896,
    /// 0.886, 0.892, 0.903, …`, a smooth basin, not noise. Smith masking at
    /// grazing incidence removes more of a *narrow* lobe than of a wide one, so
    /// a little roughness costs a grazing surface more than a lot does.
    ///
    /// From this column outward the fall is monotone to within [`NOISE`],
    /// measured: the largest rise over `x >= 16` is `4.6e-5`.
    const MONOTONE_FROM: usize = 16;

    /// How far the committed table may sit from an independent quadrature of the
    /// same integral.
    ///
    /// The reference in [`the_table_agrees_with_a_second_integration`] sweeps the
    /// hemisphere on a uniform grid instead of importance-sampling it, so the two
    /// share no arithmetic beyond the BRDF itself and disagree only by their own
    /// convergence. Measured over the six probes: at worst `7.1e-4`. Four times
    /// that.
    const QUADRATURE_TOLERANCE: f32 = 3e-3;

    /// Sides of the uniform grid [`the_table_agrees_with_a_second_integration`]
    /// integrates over, in `theta` and in `phi`.
    ///
    /// A quadrature cannot resolve a near-mirror lobe at any grid this test can
    /// afford, which is exactly why the table is importance-sampled — so the
    /// probes below are all at roughness the grid does reach.
    const QUADRATURE_STEPS: usize = 400;

    /// The whole table as `(n_dot_v_index, roughness_index, scale, bias)`.
    fn every_entry() -> impl Iterator<Item = (usize, usize, f32, f32)> {
        (0..DFG_SIZE).flat_map(|y| {
            (0..DFG_SIZE).map(move |x| {
                let [scale, bias] = entry(x, y);
                (x, y, scale, bias)
            })
        })
    }

    /// The directional albedo by uniform quadrature over the hemisphere, sharing
    /// nothing with [`bake`] but the BRDF it integrates.
    ///
    /// `∫ D V N·L dω` at `F = 1`, in `f64`. Where [`bake`] draws half-vectors
    /// from the distribution and lets the density cancel, this sweeps light
    /// directions on a grid and evaluates `D` outright — so a mistake in the
    /// importance-sampling weight, in the reflection, or in the density has
    /// nowhere to hide in both.
    fn albedo_by_quadrature(n_dot_v: f64, roughness: f64) -> f64 {
        let alpha2 = roughness.powi(4);
        let view = [(1.0 - n_dot_v * n_dot_v).max(0.0).sqrt(), 0.0, n_dot_v];
        let d_theta = std::f64::consts::FRAC_PI_2 / QUADRATURE_STEPS as f64;
        let d_phi = std::f64::consts::TAU / QUADRATURE_STEPS as f64;
        let mut total = 0.0;
        for theta_step in 0..QUADRATURE_STEPS {
            let theta = (theta_step as f64 + 0.5) * d_theta;
            let (sin_theta, n_dot_l) = (theta.sin(), theta.cos());
            for phi_step in 0..QUADRATURE_STEPS {
                let phi = (phi_step as f64 + 0.5) * d_phi;
                let light = [sin_theta * phi.cos(), sin_theta * phi.sin(), n_dot_l];
                let half = [view[0] + light[0], view[1] + light[1], view[2] + light[2]];
                let length = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
                if length < 1e-12 {
                    continue;
                }
                let n_dot_h = half[2] / length;
                if n_dot_h <= 0.0 {
                    continue;
                }
                let shape = n_dot_h * n_dot_h * (alpha2 - 1.0) + 1.0;
                let d = alpha2 / (std::f64::consts::PI * shape * shape);
                let lambda_v = n_dot_l * (n_dot_v * n_dot_v * (1.0 - alpha2) + alpha2).sqrt();
                let lambda_l = n_dot_v * (n_dot_l * n_dot_l * (1.0 - alpha2) + alpha2).sqrt();
                let visibility = 0.5 / (lambda_v + lambda_l).max(1e-12);
                total += d * visibility * n_dot_l * sin_theta * d_theta * d_phi;
            }
        }
        total
    }

    /// **No texel claims more light left than arrived.**
    #[test]
    fn every_entry_is_a_share_of_the_light_that_arrived() {
        for (x, y, scale, bias) in every_entry() {
            assert!(
                (-NOISE..=1.0 + NOISE).contains(&scale) && (-NOISE..=1.0 + NOISE).contains(&bias),
                "({x}, {y}) is ({scale}, {bias}), which is not a pair of fractions"
            );
            let albedo = scale + bias;
            assert!(
                albedo > 0.0 && albedo <= 1.0 + NOISE,
                "({x}, {y}) hands back {albedo} of the light that reached it"
            );
        }
    }

    /// **A mirror returns everything it receives**, at every angle.
    ///
    /// The smoothest row is the limit the whole table is anchored on: with no
    /// roughness there is no microsurface to bounce off twice, so nothing is
    /// lost and the compensation below has nothing to add. A table baked with
    /// the wrong sampling density fails here first, because this is the one row
    /// whose answer is known in closed form.
    #[test]
    fn a_mirror_gives_back_everything() {
        for x in 0..DFG_SIZE {
            let [scale, bias] = entry(x, 0);
            let albedo = scale + bias;
            assert!(
                (albedo - 1.0).abs() <= NOISE,
                "at N·V {} a mirror hands back {albedo}, not all of it",
                axis_value(x)
            );
        }
    }

    /// **Energy is lost as the surface roughens**, which is the whole reason
    /// this table exists.
    ///
    /// Only past [`MONOTONE_FROM`], and that constant carries the measurement of
    /// why.
    #[test]
    fn energy_is_lost_as_the_surface_roughens() {
        for x in MONOTONE_FROM..DFG_SIZE {
            for y in 0..DFG_SIZE - 1 {
                let (smoother, rougher) = (entry(x, y), entry(x, y + 1));
                let (smoother, rougher) = (smoother[0] + smoother[1], rougher[0] + rougher[1]);
                assert!(
                    rougher <= smoother + NOISE,
                    "at N·V {} roughness {} hands back {rougher} where {} hands back \
                     {smoother} — the loss does not grow with roughness",
                    axis_value(x),
                    axis_value(y + 1),
                    axis_value(y)
                );
            }
        }
    }

    /// **And it loses most of it**, which is what makes the compensation worth a
    /// table rather than a constant.
    #[test]
    fn the_roughest_surface_seen_head_on_loses_most_of_its_light() {
        let head_on = DFG_SIZE - 1;
        let roughest = directional_albedo(axis_value(head_on), axis_value(DFG_SIZE - 1));
        assert!(
            (0.25..0.40).contains(&roughest),
            "a fully rough conductor seen head-on hands back {roughest}; the \
             committed table measures 0.317, and a value outside this range is a \
             different integrand rather than a different table"
        );
        let smoothest = directional_albedo(axis_value(head_on), axis_value(0));
        assert!(
            smoothest - roughest > 0.5,
            "roughening a head-on surface costs {} of its light, which is not \
             enough to be worth compensating",
            smoothest - roughest
        );
    }

    /// **A second integration of the same lobe reaches the same numbers.**
    ///
    /// The check that says the table is right rather than merely self-consistent:
    /// [`albedo_by_quadrature`] sweeps the hemisphere on a grid where [`bake`]
    /// importance-samples it, so they share the BRDF and nothing else.
    #[test]
    fn the_table_agrees_with_a_second_integration() {
        // All at roughness a uniform grid can resolve — see `QUADRATURE_STEPS`.
        for (x, y) in [(63, 63), (63, 47), (31, 63), (31, 47), (47, 55), (15, 63)] {
            let (n_dot_v, roughness) = (axis_value(x), axis_value(y));
            let committed = directional_albedo(n_dot_v, roughness);
            let quadrature = albedo_by_quadrature(f64::from(n_dot_v), f64::from(roughness)) as f32;
            assert!(
                (committed - quadrature).abs() <= QUADRATURE_TOLERANCE,
                "at N·V {n_dot_v} roughness {roughness} the table says {committed} \
                 and a uniform quadrature says {quadrature}"
            );
        }
    }

    /// **A texel centre samples that texel and nothing else.**
    #[test]
    fn a_sample_at_a_texel_centre_is_that_texel() {
        for (x, y) in [(0, 0), (1, 7), (31, 31), (63, 0), (0, 63), (63, 63)] {
            assert_eq!(
                sample(axis_value(x), axis_value(y)),
                entry(x, y),
                "the sample at texel ({x}, {y})'s own centre is not that texel"
            );
        }
    }

    /// **The edges clamp rather than wrap**, matching the sampler a shader reads
    /// this with.
    #[test]
    fn sampling_past_the_edges_clamps_to_them() {
        assert_eq!(sample(0.0, 0.0), entry(0, 0));
        assert_eq!(sample(1.0, 1.0), entry(DFG_SIZE - 1, DFG_SIZE - 1));
        assert_eq!(sample(-4.0, 9.0), entry(0, DFG_SIZE - 1));
    }

    /// **The compensation only ever adds**, which is what makes it safe to
    /// apply to every fragment without a branch.
    #[test]
    fn the_compensation_never_takes_light_away() {
        for reflectance in [0.04f32, 0.5, 1.0] {
            for (x, y, _, _) in every_entry() {
                let gain = energy_compensation([reflectance; 3], axis_value(x), axis_value(y));
                for channel in gain {
                    assert!(
                        channel >= 1.0,
                        "f0 {reflectance} at ({x}, {y}) is scaled by {channel}, which \
                         removes light rather than restoring it"
                    );
                }
            }
        }
    }

    /// **A polished surface is left exactly alone**, so the compensation cannot
    /// move the half of a material set that had lost nothing.
    #[test]
    fn a_polished_surface_is_left_alone() {
        for x in 0..DFG_SIZE {
            let gain = energy_compensation([1.0; 3], axis_value(x), axis_value(0));
            assert!(
                (gain[0] - 1.0).abs() <= NOISE,
                "a mirror at N·V {} is scaled by {}, not left alone",
                axis_value(x),
                gain[0]
            );
        }
    }

    /// **A surface that reflects nothing gains nothing.**
    ///
    /// The multiply-scattered light left the microsurface after further bounces,
    /// and every bounce is attenuated by the surface's own reflectance — so at
    /// `f0` of zero there is nothing to come back, however rough it is.
    #[test]
    fn a_black_conductor_gains_nothing() {
        for (x, y, _, _) in every_entry() {
            let gain = energy_compensation([0.0; 3], axis_value(x), axis_value(y));
            assert_eq!(
                gain, [1.0; 3],
                "a surface reflecting nothing is scaled at ({x}, {y})"
            );
        }
    }

    /// **A white furnace closes.** At `f0` of one the compensated lobe hands
    /// back exactly what arrived, at every roughness and every angle — which is
    /// the statement "the missing energy is put back" in the one form that can
    /// be measured.
    #[test]
    fn the_compensation_closes_the_white_furnace() {
        for (x, y, scale, bias) in every_entry() {
            let restored =
                (scale + bias) * energy_compensation([1.0; 3], axis_value(x), axis_value(y))[0];
            assert!(
                (restored - 1.0).abs() <= NOISE,
                "a white surface at ({x}, {y}) hands back {restored} after \
                 compensation, not all of it"
            );
        }
    }

    /// **And the largest correction is a large one**, so a reader knows the
    /// difference is visible rather than academic.
    #[test]
    fn the_correction_is_worth_the_table_it_costs() {
        let largest = every_entry()
            .map(|(x, y, _, _)| energy_compensation([1.0; 3], axis_value(x), axis_value(y))[0])
            .fold(f32::MIN, f32::max);
        assert!(
            largest > 3.0,
            "the largest correction is {largest}, where the committed table \
             measures 3.157 — a table this flat is not the GGX lobe's"
        );
    }

    /// How far one encoded texel may sit from the number it was baked from.
    ///
    /// Half a step of the 16-bit fixed point the encoding uses, `1 / 131070`,
    /// which is `7.63e-6` — and that is exactly what the whole table measures,
    /// because rounding to nearest cannot do worse and every row has a texel
    /// that lands on the half. Stated as the bound rather than as the
    /// measurement so a change of encoding is what moves it.
    const TEXEL_TOLERANCE: f32 = 1.0 / 131_070.0;

    /// How far the image, filtered, may sit from the committed table filtered.
    ///
    /// Wider than [`TEXEL_TOLERANCE`] and not because filtering compounds the
    /// error — a convex combination cannot exceed its inputs' bound. It is the
    /// clamp: [`albedo_texels`] stores `min(scale + bias, 1)` where [`sample`]
    /// interpolates the raw pair, and the smoothest row of a Monte Carlo table
    /// sits a few parts in a hundred thousand above one. Measured worst over a
    /// 257-square sweep of both axes: `4.8e-5`. Twice that.
    const SAMPLE_TOLERANCE: f32 = 1e-4;

    /// The two axes of a sweep dense enough to land between texel centres.
    ///
    /// Odd, so that the midpoint of every texel pair is visited and the filter's
    /// worst case — both weights at a half — is actually sampled rather than
    /// stepped over.
    const SWEEP_STEPS: usize = 257;

    #[test]
    fn the_texels_encode_the_committed_table_to_a_quantisation_step() {
        let texels = albedo_texels();
        assert_eq!(
            texels.len(),
            ALBEDO_BYTES,
            "an Rg8Unorm image of a {DFG_SIZE}-square table is {ALBEDO_BYTES} bytes"
        );
        let mut worst = 0.0f32;
        for roughness in 0..DFG_SIZE {
            for n_dot_v in 0..DFG_SIZE {
                let [scale, bias] = entry(n_dot_v, roughness);
                let want = (scale + bias).clamp(0.0, 1.0);
                let index = (roughness * DFG_SIZE + n_dot_v) * ALBEDO_TEXEL_BYTES;
                let got = decode_albedo(
                    f32::from(texels[index]) / 255.0,
                    f32::from(texels[index + 1]) / 255.0,
                );
                worst = worst.max((got - want).abs());
            }
        }
        assert!(
            worst <= TEXEL_TOLERANCE,
            "a texel is {worst} from the entry it was baked from, past the {TEXEL_TOLERANCE} \
             half-step rounding to nearest allows"
        );
    }

    #[test]
    fn the_image_filters_to_the_albedo_the_table_does() {
        let mut worst = 0.0f32;
        let mut highest = f32::NEG_INFINITY;
        for i in 0..SWEEP_STEPS {
            for j in 0..SWEEP_STEPS {
                let n_dot_v = i as f32 / (SWEEP_STEPS - 1) as f32;
                let roughness = j as f32 / (SWEEP_STEPS - 1) as f32;
                let sampled = sampled_albedo(n_dot_v, roughness);
                worst = worst.max((sampled - directional_albedo(n_dot_v, roughness)).abs());
                highest = highest.max(sampled);
            }
        }
        // The guarantee the shader's compensation rests on, and the reason the
        // encoding clamps before it quantises rather than after: an albedo above
        // one inverts to a gain below one, which is a mirror *dimmed* by the term
        // that exists to brighten a rough surface.
        assert!(
            highest <= 1.0,
            "the filtered image reaches {highest}, and a share of arriving light above one is a \
             negative energy gain"
        );
        assert!(
            worst <= SAMPLE_TOLERANCE,
            "the encoded image filters to {worst} away from the committed table, past \
             {SAMPLE_TOLERANCE}"
        );
    }
}
