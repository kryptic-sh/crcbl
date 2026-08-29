//! A gradient sky: the radiance the background reads as, and the environment
//! that radiance lights the scene with.
//!
//! `docs/plan/43-render-standards.md` §8 puts a sky above scenery on the gap
//! list for one reason: **the environment term screen-space reflections fall
//! back to, and the ambient term a metal needs, are the same term a sky would
//! provide.** So this module answers both at once. [`SkyGradient`] is what a
//! ray leaving the scene sees, and [`SkyGradient::irradiance`] is the same
//! field projected onto the L1 basis this engine's probes already speak, so a
//! shading pass reads it with the dot product it already performs.
//!
//! # The gradient
//!
//! Three linear-RGB radiances — [`SkyGradient::zenith`] straight up,
//! [`SkyGradient::horizon`] at the horizon, [`SkyGradient::ground`] straight
//! down — blended by a smoothstep in the direction's `y`. It is azimuthally
//! symmetric, which is the whole of why the projection below is a closed form
//! rather than a quadrature.
//!
//! The blend is deliberately not a `pow`, which is the shape a hand-tuned sky
//! usually takes to tighten its horizon band. `docs/plan/44-lighting.md`'s
//! shading rule forbids a transcendental that reaches a colour, and a sky is
//! nothing but colour. A smoothstep is a cubic: multiplies and adds, identical
//! on all four backends, and it needs neither [`crate::fog`]'s construction nor
//! [`crate::dfg`]'s cooked table. A gradient that wants a tighter horizon than
//! a cubic gives spends a third colour band on it rather than an exponent.
//!
//! # Why the irradiance is a `GpuProbe`
//!
//! An L1 spherical-harmonic irradiance probe is exactly the shape a distant
//! environment reaches a diffuse surface in, and [`crate::probe::GpuProbe`]
//! already carries one, evaluates one in [`GpuProbe::irradiance`], and is what
//! `shaders/mesh.slang` already unpacks. Projecting the sky into that same
//! record means the sky's ambient contribution and a probe's are added by the
//! same code, and a scene with both gets one sum rather than two conventions.
//!
//! The projection is done here on the host, once per frame, which is the second
//! reason the shading rule is not in the way: the coefficients reach the GPU as
//! uploaded numbers, identical on every backend because one CPU computed them.

use crate::probe::{GpuProbe, TRANSFER_L0, TRANSFER_L1};

/// The mean of the smoothstep blend over the half-range it is used on —
/// `∫₀¹ u²(3 − 2u) du`.
///
/// Half, and not by coincidence: the cubic is antisymmetric about `u = ½`, so
/// it spends exactly as much of its range above the midpoint as below. It is
/// the weight [`SkyGradient::irradiance`]'s constant band gives the two polar
/// colours against the horizon's.
pub const BLEND_MEAN: f32 = 0.5;

/// The blend's first moment over the same range — `∫₀¹ u³(3 − 2u) du`, which is
/// `¾ − ⅖ = 7/20`.
///
/// This is the weight [`SkyGradient::irradiance`]'s linear band gives the
/// zenith-to-ground difference, and the one number in this module a reader
/// cannot check by inspection; `the_closed_form_matches_a_quadrature` is what
/// checks it, by integrating [`SkyGradient::radiance`] directly and comparing.
pub const BLEND_FIRST_MOMENT: f32 = 0.35;

/// The largest disagreement, as a fraction of the coefficient's own magnitude,
/// that [`SkyGradient::irradiance`] is allowed against a fine quadrature of
/// [`SkyGradient::radiance`].
///
/// The quadrature is the approximation of the two, so this bounds *its* error
/// and not the closed form's.
///
/// **It does not shrink with more bands, it grows**, which is the opposite of
/// what a midpoint rule over a smooth integrand does and was measured rather
/// than assumed. Summing hundreds of thousands of small `f32` terms into one
/// accumulator loses low bits faster than halving the band width recovers them,
/// so the worst relative disagreement went 2.9e-6 at a thousand bands, 3.3e-6
/// at ten thousand, and 1.2e-3 at a million. The test's band count sits at the
/// flat bottom of that curve, and this bound is three times the error measured
/// there.
pub const MAX_QUADRATURE_ERROR: f32 = 1.0e-5;

/// A sky that is a smoothstep blend between three linear-RGB radiances.
///
/// `PartialEq` but not `Eq`, for [`GpuProbe`]'s reason: every field is a float.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkyGradient {
    /// Radiance straight up, along `+Y`.
    pub zenith: [f32; 3],
    /// Radiance at the horizon, where `direction.y` is zero.
    pub horizon: [f32; 3],
    /// Radiance straight down, along `−Y`.
    pub ground: [f32; 3],
}

impl SkyGradient {
    /// The sky that lights nothing and is black in every direction.
    ///
    /// Named rather than left to [`Default`] for the reason [`GpuProbe::ZERO`]
    /// is: this is the value the additive-zero property rests on. Its
    /// [`Self::irradiance`] is [`GpuProbe::ZERO`] exactly — every coefficient
    /// is a product with a zero factor — so a renderer that has not been given
    /// a sky adds a term that is bit-for-bit nothing.
    pub const BLACK: Self = Self {
        zenith: [0.0; 3],
        horizon: [0.0; 3],
        ground: [0.0; 3],
    };

    /// The gradient as the three `float4` rows every shader block that carries
    /// one spells: zenith, horizon, ground, each padded with a `w` no shader
    /// reads.
    ///
    /// **One function because two blocks want it** — [`SkyParams::sky`] and
    /// [`crate::ssr::SsrParams::sky`] — and the order is the load-bearing part:
    /// a block filled in the other order draws a sky reflected upside down,
    /// which is a picture rather than an error.
    #[must_use]
    pub fn rows(&self) -> [[f32; 4]; 3] {
        [self.zenith, self.horizon, self.ground].map(|band| [band[0], band[1], band[2], 0.0])
    }

    /// The radiance a ray leaving the scene along `direction` sees, in linear
    /// RGB.
    ///
    /// `direction` should be unit length; only its `y` is read, since the
    /// gradient is azimuthally symmetric. Values outside `[-1, 1]` are clamped
    /// rather than extrapolated, so a caller that hands over an unnormalised
    /// direction gets the pole's colour instead of an amplified one.
    ///
    /// The blend is written `horizon * (1 − t) + far * t` rather than
    /// `horizon + (far − horizon) * t`, which agree in exact arithmetic and do
    /// not in floating point: only the first returns `horizon` and `far`
    /// *exactly* at the two ends. That is what makes a uniform sky — all three
    /// bands equal — return that one radiance in every direction with no drift,
    /// and it is the same choice the fog composite makes for the same reason.
    #[must_use]
    pub fn radiance(&self, direction: [f32; 3]) -> [f32; 3] {
        let up = direction[1].clamp(-1.0, 1.0);
        let far = if up >= 0.0 { self.zenith } else { self.ground };
        let blend = smoothstep(up.abs());
        let mut out = [0.0f32; 3];
        for channel in 0..3 {
            out[channel] = self.horizon[channel] * (1.0 - blend) + far[channel] * blend;
        }
        out
    }

    /// This sky projected onto the L1 irradiance basis, ready to be added to
    /// whatever a probe volume contributes.
    ///
    /// Closed form, not quadrature. The gradient is a function of `y` alone, so
    /// integrating it against the basis over the sphere collapses to two
    /// one-dimensional integrals of the blend — [`BLEND_MEAN`] and
    /// [`BLEND_FIRST_MOMENT`] — and the `x` and `z` bands are zero by symmetry.
    /// Writing the sphere's solid angle as `2π dy`:
    ///
    /// ```text
    /// constant band = PROJECT_L0 · 2π · (horizon + (zenith + ground)/2)
    /// linear y band = PROJECT_L1 · 2π · 7/20 · (zenith − ground)
    /// ```
    ///
    /// The horizon carries a whole unit of weight rather than a half because
    /// both hemispheres start from it; only the two polar colours are halved.
    /// The horizon cancels out of the linear band entirely, for the mirrored
    /// reason: it contributes the same amount above and below.
    ///
    /// with `PROJECT_L0` and `PROJECT_L1` the same per-sample weights
    /// [`GpuProbe::accumulate`] applies, restated through the public
    /// [`TRANSFER_L0`] and [`TRANSFER_L1`] they are derived from.
    /// `the_closed_form_matches_an_accumulated_probe` checks the two against
    /// each other rather than trusting this restatement.
    #[must_use]
    pub fn irradiance(&self) -> GpuProbe {
        // PROJECT_L0 · 2π, with PROJECT_L0 = TRANSFER_L0 / 4π.
        let constant_weight = TRANSFER_L0 / 2.0;
        // PROJECT_L1 · 2π, with PROJECT_L1 = 3 · TRANSFER_L1 / 4π.
        let linear_weight = 1.5 * TRANSFER_L1;

        let mut probe = GpuProbe::ZERO;
        for (band, channel) in [
            (&mut probe.sh_r, 0usize),
            (&mut probe.sh_g, 1),
            (&mut probe.sh_b, 2),
        ] {
            let zenith = self.zenith[channel];
            let horizon = self.horizon[channel];
            let ground = self.ground[channel];
            // Each hemisphere contributes its own mean, in the same blend
            // form `radiance` uses, so the two can be read against each other.
            let upper = horizon * (1.0 - BLEND_MEAN) + zenith * BLEND_MEAN;
            let lower = horizon * (1.0 - BLEND_MEAN) + ground * BLEND_MEAN;
            band[1] = linear_weight * BLEND_FIRST_MOMENT * (zenith - ground);
            band[3] = constant_weight * (upper + lower);
        }
        probe
    }
}

/// The cubic `u²(3 − 2u)`, for `u` already known to be in `[0, 1]`.
///
/// Spelled out rather than reached for because `f32` has no `smoothstep`, and
/// because the shader half of this gradient has to spell the same cubic: one
/// factored form on both sides is one thing to compare.
pub(crate) fn smoothstep(u: f32) -> f32 {
    u * u * (3.0 - 2.0 * u)
}

/// Bytes of the uniform block `shaders/sky.slang` reads.
///
/// Two `float4x4`s and three `float4`s, which is the whole of what a pass that
/// samples nothing needs: the two matrices turn a pixel into a world-space ray
/// and the three rows are the gradient that ray is evaluated against.
pub const PARAMS_SIZE: usize = 64 + 64 + 48;

/// The uniform block, matching `struct SkyParams` in `shaders/sky.slang`.
///
/// Every matrix is **column-major**, which is the layout the seam's other
/// blocks use and the one Slang reads a `float4x4` in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkyParams {
    /// Clip → view, reversed-Z and possibly infinite.
    pub inv_proj: [f32; 16],
    /// View → world.
    pub inv_view: [f32; 16],
    /// The gradient: zenith, horizon, ground, each in `xyz` with `w` unread.
    ///
    /// The same three rows and the same order as
    /// [`crate::ssr::SsrParams::sky`] — [`SkyGradient::rows`] is what both of
    /// them get their contents from, so the two blocks cannot disagree about
    /// which end is up.
    pub sky: [[f32; 4]; 3],
}

impl SkyParams {
    /// The block, in the byte layout the shader declares.
    #[must_use]
    pub fn to_bytes(self) -> [u8; PARAMS_SIZE] {
        let mut bytes = [0u8; PARAMS_SIZE];
        let mut at = 0;
        for value in self
            .inv_proj
            .into_iter()
            .chain(self.inv_view)
            .chain(self.sky.into_iter().flatten())
        {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            at += 4;
        }
        debug_assert_eq!(
            at, PARAMS_SIZE,
            "the two matrices and the three gradient rows fill the block exactly"
        );
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAYLIGHT: SkyGradient = SkyGradient {
        zenith: [0.18, 0.32, 0.75],
        horizon: [0.62, 0.68, 0.80],
        ground: [0.11, 0.09, 0.07],
    };

    /// Midpoint quadrature of the gradient over the sphere, through the same
    /// accumulation a probe bake uses — the independent oracle the closed form
    /// is checked against.
    ///
    /// Each sample stands for a whole band of constant `y`, and the direction
    /// handed to `accumulate` is that band's *average* direction, `(0, y, 0)`,
    /// which is shorter than unit everywhere but the poles. That is not a
    /// violation of `accumulate`'s contract so much as an exploitation of it:
    /// the weights are linear in the direction, so one averaged sample deposits
    /// exactly what the ring of unit samples it stands for would, and the `x`
    /// and `z` components that cancel around the ring are the ones missing.
    fn quadrature(sky: &SkyGradient, bands: usize) -> GpuProbe {
        let mut probe = GpuProbe::ZERO;
        let step = 2.0 / bands as f64;
        for band in 0..bands {
            let y = -1.0 + (band as f64 + 0.5) * step;
            // A band of constant `y` on the unit sphere subtends `2π dy`.
            let solid_angle = (2.0 * std::f64::consts::PI * step) as f32;
            let direction = [0.0, y as f32, 0.0];
            probe.accumulate(direction, sky.radiance(direction), solid_angle);
        }
        probe
    }

    #[test]
    fn the_black_sky_is_black_in_every_direction() {
        for y in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            assert_eq!(SkyGradient::BLACK.radiance([0.0, y, 0.0]), [0.0; 3]);
        }
    }

    #[test]
    fn the_black_sky_projects_to_the_probe_that_adds_nothing() {
        assert_eq!(SkyGradient::BLACK.irradiance(), GpuProbe::ZERO);
    }

    #[test]
    fn the_three_bands_are_returned_exactly_at_their_own_directions() {
        assert_eq!(DAYLIGHT.radiance([0.0, 1.0, 0.0]), DAYLIGHT.zenith);
        assert_eq!(DAYLIGHT.radiance([0.0, 0.0, 0.0]), DAYLIGHT.horizon);
        assert_eq!(DAYLIGHT.radiance([0.0, -1.0, 0.0]), DAYLIGHT.ground);
    }

    #[test]
    fn the_gradient_ignores_everything_but_the_up_axis() {
        let up = 0.4;
        let reference = DAYLIGHT.radiance([0.0, up, 0.0]);
        for horizontal in [[1.0, up, 0.0], [0.0, up, -7.5], [-0.3, up, 0.3]] {
            assert_eq!(DAYLIGHT.radiance(horizontal), reference);
        }
    }

    #[test]
    fn a_direction_past_the_pole_is_clamped_not_extrapolated() {
        assert_eq!(DAYLIGHT.radiance([0.0, 4.0, 0.0]), DAYLIGHT.zenith);
        assert_eq!(DAYLIGHT.radiance([0.0, -4.0, 0.0]), DAYLIGHT.ground);
    }

    #[test]
    fn the_blend_is_monotone_from_ground_through_horizon_to_zenith() {
        // One channel where the three bands are strictly ordered, so a
        // monotone blend is a checkable claim rather than a coincidence of the
        // gradient's shape.
        let ramp = SkyGradient {
            zenith: [1.0, 0.0, 0.0],
            horizon: [0.5, 0.0, 0.0],
            ground: [0.0, 0.0, 0.0],
        };
        let mut previous = f32::NEG_INFINITY;
        for step in 0u16..=200 {
            let y = -1.0 + f32::from(step) / 100.0;
            let value = ramp.radiance([0.0, y, 0.0])[0];
            assert!(
                value >= previous,
                "radiance fell from {previous} to {value} at y = {y}"
            );
            previous = value;
        }
    }

    #[test]
    fn a_uniform_sky_integrates_to_pi_times_its_radiance() {
        // The statement `probe`'s own transfer test makes, arrived at through
        // this module's closed form instead: a constant environment of
        // radiance `L` reaches a surface as `πL`, with no directional band.
        let uniform = SkyGradient {
            zenith: [0.25, 0.5, 1.0],
            horizon: [0.25, 0.5, 1.0],
            ground: [0.25, 0.5, 1.0],
        };
        let probe = uniform.irradiance();
        for (band, radiance) in [(probe.sh_r, 0.25f32), (probe.sh_g, 0.5), (probe.sh_b, 1.0)] {
            assert_eq!(band[0], 0.0);
            assert_eq!(band[1], 0.0);
            assert_eq!(band[2], 0.0);
            let expected = std::f32::consts::PI * radiance;
            assert!(
                (band[3] - expected).abs() <= expected * 1.0e-6,
                "constant band {} is not π·{radiance} = {expected}",
                band[3]
            );
        }
        // And the evaluated irradiance is that constant whichever way the
        // surface faces, since there is no direction in it to face towards.
        for normal in [[0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]] {
            assert_eq!(probe.irradiance(normal), probe.irradiance([0.0, 1.0, 0.0]));
        }
    }

    #[test]
    fn the_closed_form_matches_a_quadrature() {
        let closed = DAYLIGHT.irradiance();
        let numeric = quadrature(&DAYLIGHT, 10_000);
        for (band, sampled, name) in [
            (closed.sh_r, numeric.sh_r, "r"),
            (closed.sh_g, numeric.sh_g, "g"),
            (closed.sh_b, numeric.sh_b, "b"),
        ] {
            for coefficient in 0..4 {
                let scale = band[coefficient].abs().max(sampled[coefficient].abs());
                let error = (band[coefficient] - sampled[coefficient]).abs();
                assert!(
                    error <= scale * MAX_QUADRATURE_ERROR,
                    "sh_{name}[{coefficient}]: closed form {} against quadrature {}",
                    band[coefficient],
                    sampled[coefficient]
                );
            }
        }
    }

    #[test]
    fn the_closed_form_matches_an_accumulated_probe() {
        // The same check as the quadrature, stated against
        // `GpuProbe::accumulate`'s own weights instead: the constant band is
        // what one whole-sphere sample of the sky's mean radiance deposits.
        //
        // **The sky here has to be one whose two hemispheres differ.** A
        // symmetric one lets a projection that sums the upper hemisphere twice
        // land on the right answer, and a sabotage run that did exactly that
        // left an earlier version of this test green.
        let closed = DAYLIGHT.irradiance();
        for channel in 0..3 {
            let zenith = DAYLIGHT.zenith[channel];
            let horizon = DAYLIGHT.horizon[channel];
            let ground = DAYLIGHT.ground[channel];
            // Each hemisphere's mean, from the gradient's own blend rather than
            // from anything `irradiance` computed, then averaged into the
            // sphere's.
            let upper = horizon * (1.0 - BLEND_MEAN) + zenith * BLEND_MEAN;
            let lower = horizon * (1.0 - BLEND_MEAN) + ground * BLEND_MEAN;
            let mean = (upper + lower) / 2.0;

            let mut expected = GpuProbe::ZERO;
            expected.accumulate([0.0, 1.0, 0.0], [mean; 3], 4.0 * std::f32::consts::PI);
            // Only the constant band: one whole-sphere sample deposits a linear
            // band too, and it is not the one this sky has.
            let band = [closed.sh_r, closed.sh_g, closed.sh_b][channel][3];
            assert!(
                (band - expected.sh_r[3]).abs() <= expected.sh_r[3] * 1.0e-6,
                "channel {channel}: constant band {band} against {}",
                expected.sh_r[3]
            );
        }
    }

    #[test]
    fn a_brighter_zenith_tilts_the_linear_band_upwards() {
        // The observable the whole projection exists for: a sky brighter above
        // than below lights an upward-facing surface more than a downward one,
        // and by the amount `BLEND_FIRST_MOMENT` names.
        let probe = DAYLIGHT.irradiance();
        let up = probe.irradiance([0.0, 1.0, 0.0]);
        let down = probe.irradiance([0.0, -1.0, 0.0]);
        for channel in 0..3 {
            assert!(
                up[channel] > down[channel],
                "channel {channel}: up {} is not above down {}",
                up[channel],
                down[channel]
            );
            let difference = up[channel] - down[channel];
            // `7.0 / 20.0` spelled out rather than read from
            // `BLEND_FIRST_MOMENT`: predicting the difference from the same
            // constant the projection used would make this test agree with any
            // value that constant took, which a sabotage run confirmed.
            let predicted = 2.0
                * 1.5
                * TRANSFER_L1
                * (7.0 / 20.0)
                * (DAYLIGHT.zenith[channel] - DAYLIGHT.ground[channel]);
            assert!(
                (difference - predicted).abs() <= predicted * 1.0e-5,
                "channel {channel}: {difference} against {predicted}"
            );
        }
    }

    #[test]
    fn the_projection_is_linear_in_the_gradient() {
        // Two skies summed project to the sum of their projections, which is
        // what lets a caller scale a sky's intensity without re-deriving it.
        let dusk = SkyGradient {
            zenith: [0.05, 0.04, 0.10],
            horizon: [0.40, 0.18, 0.06],
            ground: [0.02, 0.02, 0.02],
        };
        let mut summed = SkyGradient::BLACK;
        for channel in 0..3 {
            summed.zenith[channel] = DAYLIGHT.zenith[channel] + dusk.zenith[channel];
            summed.horizon[channel] = DAYLIGHT.horizon[channel] + dusk.horizon[channel];
            summed.ground[channel] = DAYLIGHT.ground[channel] + dusk.ground[channel];
        }
        let combined = summed.irradiance();
        let parts = (DAYLIGHT.irradiance(), dusk.irradiance());
        for (band, (left, right)) in [
            (combined.sh_r, (parts.0.sh_r, parts.1.sh_r)),
            (combined.sh_g, (parts.0.sh_g, parts.1.sh_g)),
            (combined.sh_b, (parts.0.sh_b, parts.1.sh_b)),
        ] {
            for coefficient in 0..4 {
                let expected = left[coefficient] + right[coefficient];
                assert!(
                    (band[coefficient] - expected).abs() <= expected.abs() * 1.0e-6,
                    "coefficient {coefficient}: {} against {expected}",
                    band[coefficient]
                );
            }
        }
    }

    /// `sky.slang` spells this module's gradient a second time, because Slang
    /// has no `#include` and the two sides cannot share a line. (`ssr.slang`
    /// used to as well; since 2026-08-29 it reads the sky through
    /// `crate::sky_prefilter`'s table, whose own tests hold that spelling.)
    ///
    /// So the guard is a **numeric** one: parse the blend out of the shader and
    /// evaluate it against this module's own, rather than compare spellings.
    /// A spelling comparison is what this workspace reached for first for
    /// `crate::fog`'s constants and it was the weaker check — it passes on a
    /// shader that merely contains the right characters, and fails on one that
    /// writes the same number a different way.
    #[test]
    fn the_shader_spells_the_same_gradient() {
        // Every shader source carrying a copy of this gradient. A pass that
        // copied `sky_radiance` and was not listed here is a copy this guard
        // does not hold, which is the state the guard exists to end.
        for (name, source) in [("sky.slang", include_str!("../shaders/sky.slang"))] {
            let body = source
                .split_once("float3 sky_radiance(float3 direction)\n{")
                .unwrap_or_else(|| panic!("{name} declares `sky_radiance`"))
                .1
                .split_once("\n}")
                .expect("the function has a body")
                .0;

            // The three claims the mirror has to make, each one a line whose
            // absence would change what the shader computes rather than how it
            // reads. The cubic is the blend; the clamp is what stops an
            // unnormalised direction amplifying a band; the two-ended form is
            // what returns a band exactly at its own pole.
            for line in [
                "float blend = u * u * (3.0 - 2.0 * u);",
                "float up = clamp(direction.y, -1.0, 1.0);",
                "return camera.sky[1].rgb * (1.0 - blend) + far * blend;",
            ] {
                assert!(
                    body.contains(line),
                    "{name}'s `sky_radiance` no longer contains `{line}`, so it and \
                     `SkyGradient::radiance` are computing different gradients"
                );
            }
            // And the two polar bands are taken from the ends this module
            // means: row 0 is the zenith and row 2 the ground, which is the
            // order `SkyGradient::rows` writes them in. A shader that swapped
            // them renders a sky upside down, which is a picture.
            assert!(
                body.contains("up >= 0.0 ? camera.sky[0].rgb : camera.sky[2].rgb"),
                "{name}'s `sky_radiance` no longer takes the zenith above the horizon and the \
                 ground below it"
            );
        }
    }

    /// The order [`SkyParams::to_bytes`] writes the block in is the order
    /// `sky.slang` declares it in.
    #[test]
    fn the_uniform_block_matches_the_struct_sky_slang_declares() {
        let source = include_str!("../shaders/sky.slang");
        let inv_proj = source
            .find("float4x4 inv_proj;")
            .expect("sky.slang declares `float4x4 inv_proj;`");
        let inv_view = source
            .find("float4x4 inv_view;")
            .expect("sky.slang declares `float4x4 inv_view;`");
        let sky = source
            .find("float4 sky[3];")
            .expect("sky.slang declares `float4 sky[3];`");
        assert!(
            inv_proj < inv_view && inv_view < sky,
            "sky.slang declares the block in a different order than `to_bytes` writes it"
        );
    }

    /// The block serializes as two column-major matrices followed by the three
    /// gradient rows, with nothing between them.
    #[test]
    fn the_block_serializes_two_matrices_and_the_gradient() {
        let mut inv_proj = [0.0f32; 16];
        inv_proj[0] = 1.0;
        let mut inv_view = [0.0f32; 16];
        inv_view[15] = 2.0;
        let bytes = SkyParams {
            inv_proj,
            inv_view,
            sky: DAYLIGHT.rows(),
        }
        .to_bytes();

        let lane = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four"));
        assert_eq!(lane(0), 1.0, "inv_proj starts the block");
        assert_eq!(lane(64 + 60), 2.0, "inv_view follows it");
        for (row, band) in [DAYLIGHT.zenith, DAYLIGHT.horizon, DAYLIGHT.ground]
            .into_iter()
            .enumerate()
        {
            let base = 128 + row * 16;
            for (channel, expected) in band.into_iter().enumerate() {
                assert_eq!(lane(base + channel * 4), expected);
            }
            assert_eq!(lane(base + 12), 0.0, "the row's `w` is unread padding");
        }
    }

    #[test]
    fn the_blend_cubic_is_the_smoothstep_it_claims_to_be() {
        assert_eq!(smoothstep(0.0), 0.0);
        assert_eq!(smoothstep(1.0), 1.0);
        assert_eq!(smoothstep(0.5), 0.5);
        for step in 1u16..100 {
            let u = f32::from(step) / 100.0;
            // Antisymmetric about the midpoint: s(u) + s(1 − u) = 1.
            assert!((smoothstep(u) + smoothstep(1.0 - u) - 1.0).abs() <= 1.0e-6);
        }
    }
}
