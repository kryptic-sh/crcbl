//! The gradient sky convolved against the GGX lobe: the prefiltered-radiance
//! half of specular image-based lighting, as one committed table.
//!
//! `docs/plan/44-lighting.md`'s rung 3 is Karis's split-sum, and [`crate::dfg`]
//! is its BRDF half. The other half is the environment prefiltered against the
//! lobe at every roughness, which an engine with a cubemap sky stores as a mip
//! chain and bakes again for every sky. This engine's sky is
//! [`crate::sky::SkyGradient`], and a gradient has a property a cubemap does
//! not: it is **linear in its three colours** and reads nothing of a direction
//! but its `y`. A convolution is linear too, so the prefiltered radiance along
//! a reflection `R` is
//!
//! ```text
//! far · W_far + opposite · W_opposite + horizon · (1 − W_far − W_opposite)
//! ```
//!
//! with `far` the pole on `R`'s side of the horizon, `opposite` the other, and
//! two weights that depend on nothing but `(|R.y|, roughness)`. So the whole of
//! the prefilter is a 64-square, two-channel table — the `DFG` table's exact
//! shape — baked once for the lobe and committed as `tables/sky_prefilter.bin`,
//! while the sky's three colours stay the run-time parameter they are. The
//! run-time cost is one fetch and a weighted sum of three colours, which is
//! multiplies and adds and therefore what this crate's determinism rule
//! permits; the `sin`, `cos` and square root the integrator takes are spent
//! once, here, on one machine — [`crate::dfg`]'s argument, unchanged.
//!
//! [`prefiltered_radiance`] is the sum above on the CPU, and is what the
//! shader term will owe agreement with.
//!
//! # What is here, and what the rung still owes
//!
//! The table, its integrator, its sampler and the sum. The image upload, the
//! shader's specular ambient term that reads it beside the `DFG` pair, and the
//! goldens that move when a sky lights a metal are the rung's next slice;
//! `docs/backlog.md` carries them.
//!
//! Regenerate or verify with the tool that owns it:
//!
//! ```text
//! cargo run -p crcbl-shaders --example cook-sky-prefilter            # regenerate
//! cargo run -p crcbl-shaders --example cook-sky-prefilter -- --check # verify only
//! ```

use crate::dfg::hammersley;
use crate::sky::SkyGradient;

/// Texels along each axis of the table: `|R.y|` across, roughness down.
///
/// The `DFG` table's size, and for its reason: both functions are smooth in
/// both arguments, so the error a finer table would remove is below the
/// quantisation of the targets a reflection lands in. The same size is a
/// convenience for a consumer binding both and not a coupling — each table
/// names its own.
pub const PREFILTER_SIZE: usize = 64;

/// Bytes one entry occupies: two little-endian `f32`s, `W_far` then
/// `W_opposite`.
///
/// `f32` rather than a half pair, for [`crate::dfg::DFG_ENTRY_BYTES`]'s reason:
/// the table is nothing either way, and half precision would put a rounding
/// this crate cannot perform without a dependency between the integrator and
/// the committed bytes. The upload decides its own texel format, as
/// `crate::dfg::albedo_texels` does.
pub const PREFILTER_ENTRY_BYTES: usize = 8;

/// The committed table's exact length.
///
/// The artifact is typed `&[u8; PREFILTER_BYTES]` where it is included, so a
/// table of the wrong size fails to compile rather than being caught by a test.
pub const PREFILTER_BYTES: usize = PREFILTER_SIZE * PREFILTER_SIZE * PREFILTER_ENTRY_BYTES;

/// How many lobe samples [`bake`] draws per texel.
///
/// A power of two, so the base-2 radical inverse covers `[0, 1)` at exactly
/// this stratification. The residual noise at this count is measured by
/// `the_roughest_row_is_smooth_along_the_up_axis` rather than assumed.
pub const PREFILTER_SAMPLES: u32 = 1024;

/// The committed table, `tables/sky_prefilter.bin`.
///
/// In the binary, exactly as the compiled shaders are, so there is no file for
/// a deployment to lose.
const TABLE: &[u8; PREFILTER_BYTES] = include_bytes!("../tables/sky_prefilter.bin");

/// Where along an axis texel `index` sits, at the texel's centre.
///
/// Centres rather than edges, which is what a linear filter with clamped
/// addressing reads — [`crate::dfg::axis_value`]'s reason, restated for this
/// table's own size.
#[must_use]
pub fn axis_value(index: usize) -> f32 {
    (index as f32 + 0.5) / PREFILTER_SIZE as f32
}

/// The committed entry at `(up_index, roughness_index)`, as
/// `[W_far, W_opposite]`.
///
/// # Panics
///
/// If either index is at or past [`PREFILTER_SIZE`].
#[must_use]
pub fn entry(up_index: usize, roughness_index: usize) -> [f32; 2] {
    assert!(
        up_index < PREFILTER_SIZE && roughness_index < PREFILTER_SIZE,
        "({up_index}, {roughness_index}) is outside a {PREFILTER_SIZE}-square table"
    );
    let at = (roughness_index * PREFILTER_SIZE + up_index) * PREFILTER_ENTRY_BYTES;
    let far = f32::from_le_bytes([TABLE[at], TABLE[at + 1], TABLE[at + 2], TABLE[at + 3]]);
    let opposite = f32::from_le_bytes([TABLE[at + 4], TABLE[at + 5], TABLE[at + 6], TABLE[at + 7]]);
    [far, opposite]
}

/// The committed table's bytes, for a caller uploading it to a device.
///
/// Row-major, `roughness` slow and `|R.y|` fast, which is the order a 2D image
/// upload expects and the order [`entry`] indexes in.
#[must_use]
pub const fn bytes() -> &'static [u8; PREFILTER_BYTES] {
    TABLE
}

/// The table sampled bilinearly at `(|R.y|, roughness)`, the way a shader's
/// linear filter reads it, clamped at both edges rather than wrapped.
///
/// `up` is the reflection's `|y|`; a caller with a signed one takes its
/// magnitude and picks the poles itself, as [`prefiltered_radiance`] does.
#[must_use]
pub fn sample(up: f32, roughness: f32) -> [f32; 2] {
    let axis = |value: f32| {
        let scaled = value.clamp(0.0, 1.0) * PREFILTER_SIZE as f32 - 0.5;
        let low = scaled.floor().clamp(0.0, (PREFILTER_SIZE - 1) as f32);
        let high = (low + 1.0).min((PREFILTER_SIZE - 1) as f32);
        (low as usize, high as usize, (scaled - low).clamp(0.0, 1.0))
    };
    let (x0, x1, fx) = axis(up);
    let (y0, y1, fy) = axis(roughness);
    let mut out = [0.0f32; 2];
    for (channel, slot) in out.iter_mut().enumerate() {
        let top = entry(x0, y0)[channel] * (1.0 - fx) + entry(x1, y0)[channel] * fx;
        let bottom = entry(x0, y1)[channel] * (1.0 - fx) + entry(x1, y1)[channel] * fx;
        *slot = top * (1.0 - fy) + bottom * fy;
    }
    out
}

/// The sky a GGX lobe of `roughness` sees around the reflection `direction`,
/// in linear RGB: the module's sum, with the poles chosen by the direction's
/// side of the horizon.
///
/// `direction` should be unit length; only its `y` is read, and a value outside
/// `[-1, 1]` clamps to the pole rather than reading past the table. At the
/// smoothest roughness this is [`SkyGradient::radiance`] along the same
/// direction, to the table's resolution; as the lobe widens the pole's colour
/// gives way to the horizon's, which is what a rough metal facing the sky is
/// supposed to show.
///
/// **The same arithmetic the shader term will spell**, so the sum is written
/// with the horizon's weight as `1 − W_far − W_opposite` rather than stored as
/// a third channel: two channels are what the table holds, and a third that
/// had to agree with the first two to the last bit would be one more thing to
/// compare.
#[must_use]
pub fn prefiltered_radiance(sky: &SkyGradient, direction: [f32; 3], roughness: f32) -> [f32; 3] {
    let up = direction[1].clamp(-1.0, 1.0);
    let [far, opposite] = sample(up.abs(), roughness);
    let (far_band, opposite_band) = if up >= 0.0 {
        (sky.zenith, sky.ground)
    } else {
        (sky.ground, sky.zenith)
    };
    let horizon = 1.0 - far - opposite;
    let mut out = [0.0f32; 3];
    for channel in 0..3 {
        out[channel] = sky.horizon[channel] * horizon
            + far_band[channel] * far
            + opposite_band[channel] * opposite;
    }
    out
}

/// The gradient's blend `u²(3 − 2u)` in `f64`, for `u` in `[0, 1]`.
///
/// [`crate::sky::smoothstep`]'s cubic, restated in the integrator's precision
/// rather than called, because the sum below runs over [`PREFILTER_SAMPLES`]
/// terms and rounds to `f32` once at the end. `the_blend_is_the_skys_own_cubic`
/// holds the two to each other.
fn blend(u: f64) -> f64 {
    u * u * (3.0 - 2.0 * u)
}

/// Integrate the sky's blend weights against the GGX lobe and return the table
/// row-major with `roughness` slow.
///
/// Karis's prefilter, for a reflection `R` at `|R.y|` from the horizon and the
/// view and normal both taken along it — the split-sum's standing assumption:
///
/// 1. Importance-sample a half-vector from the Trowbridge-Reitz distribution
///    about `R`, whose `cos θ` is `sqrt((1 - ξ) / (1 + (α² - 1) ξ))`, and
///    reflect `R` about it to get the light direction.
/// 2. Skip a light below the surface, and weight the rest by `N·L` — the
///    prefilter's weight, which sharpens the lobe's tail where the sampling
///    density alone would not.
/// 3. Read the gradient's three weights at the light's `y`: the pole on `R`'s
///    side gets `blend(|y|)` when the light is on that side, the opposite pole
///    gets it when the light has crossed the horizon, and the horizon gets the
///    rest. Accumulate each, and divide by the summed weight.
///
/// The frame about `R` is `(R.y, −sqrt(1 − R.y²), 0)` and `(0, 0, 1)`, which
/// is orthonormal for an `R` in the `xy` plane — the only plane it needs, since
/// the gradient is azimuthally symmetric and so is the lobe about `R`. In
/// `f64` throughout, rounded to `f32` once at the end.
#[must_use]
pub fn bake() -> Vec<[f32; 2]> {
    let mut table = Vec::with_capacity(PREFILTER_SIZE * PREFILTER_SIZE);
    for roughness_index in 0..PREFILTER_SIZE {
        let roughness = f64::from(axis_value(roughness_index));
        let alpha = roughness * roughness;
        let alpha2 = alpha * alpha;
        for up_index in 0..PREFILTER_SIZE {
            let up = f64::from(axis_value(up_index));
            let side = (1.0 - up * up).max(0.0).sqrt();
            let reflection = [side, up, 0.0];
            let tangent = [up, -side, 0.0];
            let bitangent = [0.0, 0.0, 1.0];
            let mut far = 0.0f64;
            let mut opposite = 0.0f64;
            let mut total = 0.0f64;
            for i in 0..PREFILTER_SAMPLES {
                let (u, v) = hammersley(i, PREFILTER_SAMPLES);
                let phi = std::f64::consts::TAU * u;
                let cos_theta = ((1.0 - v) / (1.0 + (alpha2 - 1.0) * v)).max(0.0).sqrt();
                let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                let local = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                let mut half = [0.0f64; 3];
                for axis in 0..3 {
                    half[axis] = tangent[axis] * local[0]
                        + bitangent[axis] * local[1]
                        + reflection[axis] * local[2];
                }
                // `R·H` is the local `z`, since `R` is the frame's third axis.
                let r_dot_h = local[2];
                let light_y = 2.0 * r_dot_h * half[1] - reflection[1];
                let n_dot_l = 2.0 * r_dot_h * r_dot_h - 1.0;
                if n_dot_l <= 0.0 {
                    continue;
                }
                let weight = n_dot_l;
                let pole = blend(light_y.abs().min(1.0)) * weight;
                if light_y >= 0.0 {
                    far += pole;
                } else {
                    opposite += pole;
                }
                total += weight;
            }
            table.push([(far / total) as f32, (opposite / total) as f32]);
        }
    }
    table
}

/// [`bake`]'s output as the bytes `tables/sky_prefilter.bin` holds.
#[must_use]
pub fn bake_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(PREFILTER_BYTES);
    for [far, opposite] in bake() {
        bytes.extend_from_slice(&far.to_le_bytes());
        bytes.extend_from_slice(&opposite.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky::smoothstep;

    /// How far a weight may sit outside its range, or a pair's sum past one,
    /// before it stops being a share of the lobe.
    ///
    /// The weights are ratios of sums of positive terms, so the only slack is
    /// the `f32` rounding of each — a step or two in the last place.
    const NOISE: f32 = 1e-6;

    /// How far the smoothest row may sit from the gradient's own blend.
    ///
    /// At the smoothest roughness the lobe has an `α²` of `3.7e-9` and reads
    /// the sky in one direction, so the row is the blend at the texel's `up`,
    /// to within the Monte Carlo residue of a lobe that narrow. Measured over
    /// the committed row: at worst `1.6e-6`, and nothing from across the
    /// horizon at all. Ten times that.
    const MIRROR_TOLERANCE: f32 = 2e-5;

    /// The largest step against the trend between neighbouring `up` texels of
    /// the roughest row that the estimator's noise is allowed to explain.
    ///
    /// The true function is smooth and monotone along that row, so a step
    /// against the trend is noise. Measured over the committed row: there is
    /// none — every step rises — so this is [`NOISE`], the rounding slack, and
    /// not a Monte Carlo bound.
    const ROUGH_NOISE: f32 = NOISE;

    /// How far the committed table may sit from an independent quadrature.
    ///
    /// [`weights_by_quadrature`] sweeps the half-vector hemisphere on a grid and
    /// evaluates the distribution outright where [`bake`] draws from it, so the
    /// two share the blend and the lobe and nothing else. Measured over the six
    /// probes: at worst `5.9e-4`. Five times that.
    const QUADRATURE_TOLERANCE: f32 = 3e-3;

    /// Sides of [`weights_by_quadrature`]'s grid, in `θ` and `φ`.
    const QUADRATURE_STEPS: usize = 400;

    /// A sky with three distinct, nameable colours.
    const SKY: SkyGradient = SkyGradient {
        zenith: [0.1, 0.2, 0.9],
        horizon: [0.8, 0.7, 0.6],
        ground: [0.3, 0.2, 0.1],
    };

    fn every_entry() -> impl Iterator<Item = (usize, usize, f32, f32)> {
        (0..PREFILTER_SIZE).flat_map(|y| {
            (0..PREFILTER_SIZE).map(move |x| {
                let [far, opposite] = entry(x, y);
                (x, y, far, opposite)
            })
        })
    }

    /// The two weights by uniform quadrature over the half-vector hemisphere
    /// about `R`, sharing nothing with [`bake`] but the blend and the lobe.
    ///
    /// The prefilter is `∫ w(L) N·L D(H) N·H dω_H / ∫ N·L D(H) N·H dω_H` with
    /// `N = V = R` — the density [`bake`] draws from, evaluated outright — so a
    /// mistake in the sampling transform, the frame or the reflection has
    /// nowhere to hide in both.
    fn weights_by_quadrature(up: f64, roughness: f64) -> [f64; 2] {
        let alpha2 = roughness.powi(4);
        let side = (1.0 - up * up).max(0.0).sqrt();
        let reflection = [side, up, 0.0];
        let tangent = [up, -side, 0.0];
        let bitangent = [0.0, 0.0, 1.0];
        let d_theta = std::f64::consts::FRAC_PI_2 / QUADRATURE_STEPS as f64;
        let d_phi = std::f64::consts::TAU / QUADRATURE_STEPS as f64;
        let (mut far, mut opposite, mut total) = (0.0, 0.0, 0.0);
        for theta_step in 0..QUADRATURE_STEPS {
            let theta = (theta_step as f64 + 0.5) * d_theta;
            let (sin_theta, cos_theta) = (theta.sin(), theta.cos());
            let shape = cos_theta * cos_theta * (alpha2 - 1.0) + 1.0;
            let d = alpha2 / (std::f64::consts::PI * shape * shape);
            let n_dot_l = 2.0 * cos_theta * cos_theta - 1.0;
            if n_dot_l <= 0.0 {
                continue;
            }
            for phi_step in 0..QUADRATURE_STEPS {
                let phi = (phi_step as f64 + 0.5) * d_phi;
                let local = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                let half_y =
                    tangent[1] * local[0] + bitangent[1] * local[1] + reflection[1] * local[2];
                let light_y = 2.0 * cos_theta * half_y - reflection[1];
                let weight = d * cos_theta * n_dot_l * sin_theta * d_theta * d_phi;
                let pole = blend(light_y.abs().min(1.0)) * weight;
                if light_y >= 0.0 {
                    far += pole;
                } else {
                    opposite += pole;
                }
                total += weight;
            }
        }
        [far / total, opposite / total]
    }

    /// **Every entry is a pair of shares that leave room for the horizon.**
    #[test]
    fn every_entry_is_a_share_of_the_lobe() {
        for (x, y, far, opposite) in every_entry() {
            assert!(
                (-NOISE..=1.0 + NOISE).contains(&far)
                    && (-NOISE..=1.0 + NOISE).contains(&opposite)
                    && far + opposite <= 1.0 + NOISE,
                "({x}, {y}) is ({far}, {opposite}), which is not a pair of shares"
            );
        }
    }

    /// **A mirror sees the gradient itself**: the smoothest row is the blend at
    /// its own `up`, and nothing from across the horizon.
    #[test]
    fn a_mirror_sees_the_gradient_itself() {
        for x in 0..PREFILTER_SIZE {
            let [far, opposite] = entry(x, 0);
            let expected = smoothstep(axis_value(x));
            assert!(
                (far - expected).abs() <= MIRROR_TOLERANCE && opposite.abs() <= MIRROR_TOLERANCE,
                "at up {} the mirror row is ({far}, {opposite}) where the gradient is {expected}",
                axis_value(x)
            );
        }
    }

    /// **Roughness trades the pole for the horizon.** Looking straight up, the
    /// zenith's weight falls monotonically as the lobe widens, and the roughest
    /// lobe still sees more zenith than horizon — it is centred on the zenith.
    #[test]
    fn roughness_trades_the_pole_for_the_horizon() {
        let top = PREFILTER_SIZE - 1;
        let mut previous = entry(top, 0)[0];
        for y in 1..PREFILTER_SIZE {
            let [far, _] = entry(top, y);
            assert!(
                far <= previous + NOISE,
                "at roughness {} the zenith's weight rose from {previous} to {far}",
                axis_value(y)
            );
            previous = far;
        }
        let [roughest, opposite] = entry(top, top);
        assert!(
            roughest < 1.0 - 0.1 && roughest > 0.5,
            "the roughest lobe facing up sees {roughest} of the zenith"
        );
        assert!(
            opposite < 0.05,
            "the roughest lobe facing up sees {opposite} of the ground, which is behind it"
        );
    }

    /// **The roughest row is smooth along the `up` axis**, which is the
    /// measurement [`PREFILTER_SAMPLES`] rests on: the pole's weight rises with
    /// `up` and never dips against that trend by more than the noise bound.
    #[test]
    fn the_roughest_row_is_smooth_along_the_up_axis() {
        let y = PREFILTER_SIZE - 1;
        for x in 1..PREFILTER_SIZE {
            let (before, after) = (entry(x - 1, y)[0], entry(x, y)[0]);
            assert!(
                after >= before - ROUGH_NOISE,
                "between up {} and {} the roughest row fell from {before} to {after}",
                axis_value(x - 1),
                axis_value(x)
            );
        }
    }

    /// The check that says the table is right rather than self-consistent.
    #[test]
    fn the_table_agrees_with_a_second_integration() {
        // All at roughness a uniform grid can resolve — see `QUADRATURE_STEPS`.
        for (x, y) in [(63, 63), (63, 47), (31, 63), (31, 47), (0, 55), (15, 63)] {
            let (up, roughness) = (axis_value(x), axis_value(y));
            let committed = entry(x, y);
            let quadrature = weights_by_quadrature(f64::from(up), f64::from(roughness));
            for channel in 0..2 {
                assert!(
                    (committed[channel] - quadrature[channel] as f32).abs() <= QUADRATURE_TOLERANCE,
                    "at up {up} roughness {roughness} the table says {committed:?} \
                     and a uniform quadrature says {quadrature:?}"
                );
            }
        }
    }

    /// **The blend is the sky's own cubic**, so the table describes the
    /// gradient the shader draws and not a neighbour of it. To a last place:
    /// the two are one polynomial in two precisions, and rounding the `f64`
    /// once is not the `f32`'s three roundings.
    #[test]
    fn the_blend_is_the_skys_own_cubic() {
        for step in 0..=20 {
            let u = step as f32 / 20.0;
            let (wide, narrow) = (blend(f64::from(u)) as f32, smoothstep(u));
            assert!(
                (wide - narrow).abs() <= f32::EPSILON,
                "at {u}: {wide} against {narrow}"
            );
        }
    }

    /// **A texel centre samples that texel and nothing else**, and the edges
    /// clamp rather than wrap.
    #[test]
    fn a_sample_at_a_texel_centre_is_that_texel_and_the_edges_clamp() {
        for (x, y) in [(0, 0), (1, 7), (31, 31), (63, 0), (0, 63), (63, 63)] {
            assert_eq!(
                sample(axis_value(x), axis_value(y)),
                entry(x, y),
                "texel ({x}, {y})"
            );
        }
        assert_eq!(sample(0.0, 0.0), entry(0, 0));
        assert_eq!(
            sample(1.0, 1.0),
            entry(PREFILTER_SIZE - 1, PREFILTER_SIZE - 1)
        );
        assert_eq!(sample(-4.0, 9.0), entry(0, PREFILTER_SIZE - 1));
    }

    /// **A uniform sky is that colour at every roughness, and a black one is
    /// black exactly.**
    #[test]
    fn a_uniform_sky_is_that_colour_and_a_black_sky_is_black() {
        let grey = SkyGradient {
            zenith: [0.5; 3],
            horizon: [0.5; 3],
            ground: [0.5; 3],
        };
        for roughness in [0.0f32, 0.3, 1.0] {
            for direction in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.6, -0.8, 0.0]] {
                let seen = prefiltered_radiance(&grey, direction, roughness);
                for channel in seen {
                    assert!((channel - 0.5).abs() <= 1e-6, "{seen:?} at {roughness}");
                }
            }
        }
        assert_eq!(
            prefiltered_radiance(&SkyGradient::BLACK, [0.0, 1.0, 0.0], 0.5),
            [0.0; 3]
        );
    }

    /// **A mirror reflects the gradient's radiance**, above and below the
    /// horizon, and a rough surface facing the zenith sees the horizon mixed
    /// in.
    #[test]
    fn a_mirror_reflects_the_gradient_and_a_rough_surface_sees_the_horizon() {
        for direction in [
            [0.0, 1.0, 0.0],
            [0.8, 0.6, 0.0],
            [0.0, -1.0, 0.0],
            [0.6, -0.8, 0.0],
        ] {
            let seen = prefiltered_radiance(&SKY, direction, 0.0);
            let radiance = SKY.radiance(direction);
            for channel in 0..3 {
                assert!(
                    (seen[channel] - radiance[channel]).abs() <= 1e-3,
                    "along {direction:?} a mirror sees {seen:?} where the sky is {radiance:?}"
                );
            }
        }
        let rough = prefiltered_radiance(&SKY, [0.0, 1.0, 0.0], 1.0);
        // The zenith is blue and the horizon warm: a rough surface facing up
        // has more red than the zenith and less blue than it.
        assert!(
            rough[0] > SKY.zenith[0] && rough[2] < SKY.zenith[2],
            "{rough:?}"
        );
    }

    /// **Below the horizon mirrors above**: a sky with its poles swapped, seen
    /// along the mirrored direction, is the same colour — which is the sign
    /// handling in [`prefiltered_radiance`], since the table itself holds only
    /// the upper half.
    #[test]
    fn below_the_horizon_mirrors_above() {
        let flipped = SkyGradient {
            zenith: SKY.ground,
            horizon: SKY.horizon,
            ground: SKY.zenith,
        };
        for roughness in [0.0f32, 0.5, 1.0] {
            for direction in [[0.0, 1.0, 0.0], [0.8, 0.6, 0.0], [0.99, 0.14, 0.0]] {
                let mirrored = [direction[0], -direction[1], direction[2]];
                assert_eq!(
                    prefiltered_radiance(&SKY, direction, roughness),
                    prefiltered_radiance(&flipped, mirrored, roughness),
                    "along {direction:?} at {roughness}"
                );
            }
        }
    }
}
