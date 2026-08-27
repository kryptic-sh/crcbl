//! Single scattering in a homogeneous slice of participating medium: how much
//! light a slab of fog sends toward the eye, and how much of what is behind it
//! survives.
//!
//! [`crate::fog`] answers the second question alone, in closed form, for a
//! medium that only *absorbs*. This module is the other half of the standard
//! froxel volumetric pass described in `docs/plan/43-render-standards.md` §4:
//! the frustum is cut into slices along `z`, each slice is shaded once against
//! the lights its froxel already lists, and the column is composited front to
//! back. What a slice owes that composite is two numbers —
//!
//! ```text
//! scattering    = the radiance the slice itself adds, self-attenuated
//! transmittance = the fraction of everything behind it that gets through
//! ```
//!
//! — and [`integrate_slice`] is both, in one closed form. [`phase`] is the
//! angular half: how a scattering event redistributes light, which is what
//! makes fog glow around a light rather than uniformly.
//!
//! # The integral, and why it is not `source * thickness`
//!
//! A slice of extinction `sigma` and thickness `dt` scatters `source` per unit
//! length toward the eye, but light scattered at the *far* end of the slice
//! still has to cross the rest of the slice to leave it. Integrating that:
//!
//! ```text
//! L = the integral of source * exp(-sigma * s) ds, over s in [0, dt]
//!   = source * (1 - exp(-sigma * dt)) / sigma
//!   = source * dt * one_minus_exp_over(sigma * dt)
//! ```
//!
//! The last form is the one written here, for the reason
//! [`crate::fog::one_minus_exp_over`] exists: as `sigma * dt` goes to zero the
//! middle form is a division of nothing by nothing, and a thin slice is the
//! common case in a froxel grid rather than the edge one. Taking `source * dt`
//! instead — the form that ignores self-attenuation — is the classic way a
//! volumetric pass gets brighter every time someone adds slices, because the
//! error is per slice and there are more of them.
//!
//! `splitting_a_slice_composites_to_the_same_radiance` is what pins that: a
//! homogeneous column cut into any number of slices composites to what one
//! slice of the whole depth gives.
//!
//! # The shading rule
//!
//! `docs/plan/44-lighting.md`: **no transcendental function may reach a
//! colour.** Neither of these two calls one. The exponential is
//! [`crate::fog::exp_neg`], built from operations IEEE-754 pins down; and
//! [`phase`]'s three-halves power is written `d * sqrt(d)` rather than
//! `pow(d, 1.5)`, because IEEE-754 requires a correctly rounded `sqrt` and
//! specifies nothing at all about `pow`.

use crate::fog::{exp_neg, one_minus_exp_over};

/// `1 / (4 pi)`, the phase function of a medium that scatters every direction
/// alike — and the factor every other phase function is a redistribution of.
///
/// Named rather than spelled at the call site because the Slang mirror has to
/// carry it as a literal, there being no `#include` in these shaders, and a
/// named constant is what a drift guard can hold that literal against.
pub const INV_FOUR_PI: f32 = 0.079_577_47;

/// The largest anisotropy [`phase`] will accept, in either direction.
///
/// At `|g| = 1` the Henyey-Greenstein lobe is a delta function and the
/// denominator is exactly zero in the forward direction, so the clamp is what
/// keeps a division by zero out of a colour rather than a tuning knob. It sits
/// well past any medium worth naming — Mie scattering in fog is around `0.8`,
/// smoke lower still — and the lobe it admits is finite and still integrates
/// to one, which `anisotropy_is_clamped_short_of_the_singularity` checks at
/// the clamp itself.
pub const MAX_ANISOTROPY: f32 = 0.99;

/// The Henyey-Greenstein phase function: the fraction of light scattered from
/// one direction into another, per steradian.
///
/// `cos_theta` is the cosine of the angle between the direction the light was
/// travelling and the direction it leaves in, so `1.0` is straight on and
/// `-1.0` is straight back. `g` is the anisotropy — positive scatters forward,
/// zero scatters evenly, negative scatters back — and is clamped to
/// [`MAX_ANISOTROPY`].
///
/// ```text
/// p(g, c) = (1 - g^2) / (4 pi * d * sqrt(d)),   d = 1 + g^2 - 2 g c
/// ```
///
/// It integrates to one over the sphere for every `g`, which is what makes a
/// medium redistribute light rather than create or destroy it, and is checked
/// by `the_phase_integrates_to_one_over_the_sphere`.
#[must_use]
pub fn phase(g: f32, cos_theta: f32) -> f32 {
    let g = g.clamp(-MAX_ANISOTROPY, MAX_ANISOTROPY);
    let denominator = 1.0 + g * g - 2.0 * g * cos_theta.clamp(-1.0, 1.0);
    INV_FOUR_PI * (1.0 - g * g) / (denominator * denominator.sqrt())
}

/// What one slice of medium contributes to the column it sits in.
///
/// Both fields are per colour channel where the caller's medium is coloured;
/// [`integrate_slice`] takes and returns scalars, and a caller with a coloured
/// medium calls it once per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliceIntegral {
    /// The radiance this slice adds, already attenuated by the part of itself
    /// in front of each scattering event.
    pub scattering: f32,
    /// The fraction of the radiance arriving from behind this slice that
    /// leaves the front of it.
    pub transmittance: f32,
}

/// Integrates single scattering across one homogeneous slice.
///
/// `source` is the radiance scattered toward the eye per unit length — the
/// product of the medium's scattering coefficient, [`phase`] at this froxel's
/// geometry, and the light reaching it. `extinction` is the medium's total
/// extinction coefficient, absorption plus scattering, per unit length.
/// `thickness` is how deep the slice is along the view ray.
///
/// A caller composites a column front to back:
///
/// ```
/// use crcbl_shaders::volumetric::integrate_slice;
///
/// let mut radiance = 0.0;
/// let mut through = 1.0_f32;
/// for _ in 0..4 {
///     let slice = integrate_slice(2.0, 0.5, 0.25);
///     radiance += through * slice.scattering;
///     through *= slice.transmittance;
/// }
/// // Which is the same column one slice deep, to within a rounding step.
/// let whole = integrate_slice(2.0, 0.5, 1.0);
/// assert!((radiance - whole.scattering).abs() < 1e-6);
/// ```
///
/// A non-positive `extinction` or `thickness` is a medium that does nothing:
/// the slice transmits exactly one and adds `source * thickness`, which is the
/// limit the integral approaches rather than a special case.
#[must_use]
pub fn integrate_slice(source: f32, extinction: f32, thickness: f32) -> SliceIntegral {
    let thickness = thickness.max(0.0);
    let depth = extinction.max(0.0) * thickness;
    SliceIntegral {
        scattering: source * thickness * one_minus_exp_over(depth),
        transmittance: exp_neg(depth),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anisotropies that cover both lobes, the isotropic case, and the clamp.
    const ANISOTROPIES: [f32; 7] = [-0.9, -0.5, -0.1, 0.0, 0.3, 0.8, MAX_ANISOTROPY];

    /// `2 pi` times Simpson's rule over `cos_theta`, which is the solid-angle
    /// integral of any function that depends on direction only through it.
    ///
    /// `f64` throughout and a step far finer than the lobe: a test is not
    /// shading, so the rule this module's construction exists to satisfy does
    /// not bind here.
    fn over_the_sphere(g: f32) -> f64 {
        const PANELS: usize = 200_000;
        let at = |c: f64| f64::from(phase(g, c as f32));
        let step = 2.0 / PANELS as f64;
        let mut total = at(-1.0) + at(1.0);
        for panel in 1..PANELS {
            let c = -1.0 + step * panel as f64;
            total += at(c) * if panel % 2 == 0 { 2.0 } else { 4.0 };
        }
        total * step / 3.0 * 2.0 * core::f64::consts::PI
    }

    /// An isotropic medium scatters every direction alike, and the constant it
    /// scatters them by is the one the sphere's solid angle names.
    ///
    /// Exactly, not nearly: at `g = 0` the whole numerator and denominator are
    /// one, so a construction that reached this value by any other route than
    /// the reciprocal it claims would show up here.
    #[test]
    fn an_isotropic_phase_is_the_reciprocal_of_the_sphere() {
        for step in 0..=20u8 {
            let cos_theta = -1.0 + f32::from(step) * 0.1;
            assert_eq!(
                phase(0.0, cos_theta),
                INV_FOUR_PI,
                "an isotropic lobe varied with direction at {cos_theta}"
            );
        }
        let ulps = (f64::from(INV_FOUR_PI) - 1.0 / (4.0 * core::f64::consts::PI)).abs()
            / f64::from(f32::EPSILON / 2.0)
            * 4.0
            * core::f64::consts::PI;
        assert!(ulps <= 1.0, "INV_FOUR_PI is {ulps} ulp from 1/(4 pi)");
    }

    /// Every lobe redistributes light rather than making or destroying it.
    ///
    /// This is the property the whole model rests on, and the one a mistyped
    /// normalisation breaks silently: dropping the `1 - g^2` factor still
    /// gives a plausible forward lobe, and still tonemaps to something that
    /// looks like fog.
    #[test]
    fn the_phase_integrates_to_one_over_the_sphere() {
        for g in ANISOTROPIES {
            let total = over_the_sphere(g);
            assert!(
                (total - 1.0).abs() < 1e-3,
                "a lobe at g = {g} integrated to {total}, not one"
            );
        }
    }

    /// Reversing the anisotropy mirrors the lobe, exactly.
    ///
    /// The sign of `g` is the whole difference between fog that glows around a
    /// light and fog that glows away from one, and this is the cheapest way to
    /// catch it having been folded into the denominator the wrong way round.
    #[test]
    fn the_sign_of_g_mirrors_the_lobe() {
        for g in ANISOTROPIES {
            for step in 0..=20u8 {
                let cos_theta = -1.0 + f32::from(step) * 0.1;
                assert_eq!(
                    phase(g, cos_theta),
                    phase(-g, -cos_theta),
                    "the lobe at g = {g} was not the mirror of the one at {}",
                    -g
                );
            }
        }
    }

    /// A positive anisotropy peaks straight ahead and falls off all the way
    /// back, with no flat stretch anywhere in between.
    ///
    /// Monotonicity is what rules out the denominator's two `g` terms having
    /// swapped places: `1 + g^2 - 2 g c` and `1 + g^2 + 2 g c` both peak, and
    /// they peak at opposite ends.
    #[test]
    fn a_forward_lobe_peaks_ahead_and_falls_off_behind() {
        for g in [0.3, 0.8, MAX_ANISOTROPY] {
            let mut previous = phase(g, -1.0);
            for step in 1..=200u8 {
                let cos_theta = -1.0 + f32::from(step) * 0.01;
                let here = phase(g, cos_theta);
                assert!(
                    here > previous,
                    "the lobe at g = {g} did not rise at {cos_theta}: {here} after {previous}"
                );
                previous = here;
            }
            assert!(
                phase(g, 1.0) > phase(g, -1.0) * 4.0,
                "a lobe at g = {g} was barely forward at all"
            );
        }
    }

    /// The singularity at `|g| = 1` is not reachable, and what the clamp
    /// admits is still a phase function.
    ///
    /// A caller handed the anisotropy straight through would divide by zero
    /// looking straight down a forward lobe, and the frame would carry an
    /// infinity from a value that reads as physical.
    #[test]
    fn anisotropy_is_clamped_short_of_the_singularity() {
        for g in [1.0, -1.0, 4.0, -4.0, f32::MAX] {
            let peak = phase(g, g.signum());
            assert!(peak.is_finite(), "g = {g} produced {peak} straight ahead");
            assert_eq!(
                peak,
                phase(g.signum() * MAX_ANISOTROPY, g.signum()),
                "g = {g} was not clamped to MAX_ANISOTROPY"
            );
        }
        let total = over_the_sphere(MAX_ANISOTROPY);
        assert!(
            (total - 1.0).abs() < 1e-3,
            "the lobe at the clamp integrated to {total}, not one"
        );
    }

    /// A medium with no extinction hides nothing and attenuates nothing, so
    /// its slice is its source over its length and no less.
    ///
    /// Exact on both counts: `exp_neg(0.0)` is `1.0` and
    /// `one_minus_exp_over(0.0)` is `1.0`, which is what makes switching the
    /// medium off a no-op rather than nearly one.
    #[test]
    fn a_slice_of_nothing_is_its_source_over_its_length() {
        for thickness in [0.0, 0.25, 1.0, 40.0] {
            let slice = integrate_slice(3.0, 0.0, thickness);
            assert_eq!(slice.transmittance, 1.0);
            assert_eq!(slice.scattering, 3.0 * thickness);
        }
        // And a negative coefficient is the same medium, not an amplifier.
        let negative = integrate_slice(3.0, -2.0, 1.0);
        assert_eq!(negative, integrate_slice(3.0, 0.0, 1.0));
        assert_eq!(
            integrate_slice(3.0, 2.0, -1.0),
            integrate_slice(3.0, 2.0, 0.0)
        );
    }

    /// Cutting a column into more slices does not change what it looks like.
    ///
    /// **The property the whole integral exists for.** Dropping the
    /// self-attenuation — `source * thickness` alone, which is the form that
    /// reads correctly and is what a froxel pass reaches for first — passes
    /// every other test in this module and fails only this one, and it fails
    /// it in the direction that matters: more slices, more light.
    #[test]
    fn splitting_a_slice_composites_to_the_same_radiance() {
        for (source, extinction, depth) in [
            (2.0_f32, 0.5_f32, 1.0_f32),
            (0.25, 4.0, 8.0),
            (10.0, 0.001, 0.5),
            (1.0, 1.0, 30.0),
        ] {
            let whole = integrate_slice(source, extinction, depth);
            for slices in [1_u32, 2, 7, 64, 512] {
                let thickness = depth / slices as f32;
                let mut radiance = 0.0_f32;
                let mut through = 1.0_f32;
                for _ in 0..slices {
                    let slice = integrate_slice(source, extinction, thickness);
                    radiance += through * slice.scattering;
                    through *= slice.transmittance;
                }
                let error = (radiance - whole.scattering).abs() / whole.scattering.max(1e-6);
                assert!(
                    error < 1e-3,
                    "{slices} slices of a column {depth} deep gave {radiance}, \
                     against {} for one",
                    whole.scattering
                );
                let transmitted = (through - whole.transmittance).abs();
                assert!(
                    transmitted < 1e-4,
                    "{slices} slices transmitted {through}, against {} for one",
                    whole.transmittance
                );
            }
        }
    }

    /// What a slice scatters and what it hides are the same integral read two
    /// ways: a slice that lets nothing through has scattered every unit of
    /// source it could.
    ///
    /// `1 - T = sigma * dt * one_minus_exp_over(sigma * dt)`, so a unit source
    /// in a medium that scatters everything it extinguishes gives back exactly
    /// the light the medium took out. An integral off by any factor at all
    /// separates the two sides.
    ///
    /// The oracle is `f64`, not `1.0 - slice.transmittance`. That difference
    /// is the cancellation [`crate::fog::one_minus_exp_over`] exists to avoid
    /// — at an optical depth of `1e-6` it keeps four significant digits — so
    /// checking against it would be holding the accurate side to the
    /// inaccurate one.
    #[test]
    fn a_slice_scatters_exactly_what_it_extinguishes() {
        for extinction in [0.001_f32, 0.1, 1.0, 5.0, 40.0] {
            for thickness in [0.001_f32, 0.05, 1.0, 3.0] {
                let slice = integrate_slice(extinction, extinction, thickness);
                let hidden = -(-f64::from(extinction) * f64::from(thickness)).exp_m1();
                let error = (f64::from(slice.scattering) - hidden).abs() / hidden;
                assert!(
                    error < 1e-5,
                    "a slice of {extinction} over {thickness} scattered {} \
                     while hiding {hidden}",
                    slice.scattering
                );
            }
        }
    }
}
