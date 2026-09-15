//! The four medium presets: what the courtyard's water is made of.
//!
//! `docs/plan/sample/21-tide.md` names pond and swamp as *medium presets* of a
//! lake rather than as body kinds of their own, and a clear pool is the
//! courtyard's own water. Each is a [`Medium`] — per-channel absorption and
//! scattering in 1/m, `crcbl-water`'s parameter set — and nothing else: the
//! presets differ in what the water holds, never in how it is drawn.
//!
//! # Where the numbers come from
//!
//! One model for all four, so the presets are four points on one line rather
//! than four colours somebody liked. Each channel stands for one wavelength —
//! red 650 nm, green 550 nm, blue 450 nm — and each coefficient is a sum:
//!
//! ```text
//! absorption(λ) = a_w(λ) + a_g(440) · exp(−S · (λ − 440))
//! scattering(λ) = b_w(λ) + b_p(550) · (550 / λ)
//! ```
//!
//! * **`a_w`, pure water's absorption**, is Pope and Fry's integrating-cavity
//!   measurement (Applied Optics 36, 8710–8723, 1997): 0.340, 0.0565 and
//!   0.0092 per metre at the three wavelengths — [`PURE_WATER_ABSORPTION`].
//! * **`b_w`, pure water's scattering**, is Morel's (1974) volume scattering
//!   function for pure water, `β(90°) = 1.38 × 10⁻⁴ (λ/500)^−4.32` per metre
//!   per steradian with a depolarisation ratio of 0.09, integrated over the
//!   sphere: `b_w(500) = 0.00222` per metre.
//! * **`a_g`, dissolved organic matter**, is the exponential Bricaud, Morel and
//!   Prieur (Limnology and Oceanography 26, 43–53, 1981) fitted to it, with
//!   their slope `S = 0.014` per nanometre. It is what stains a pond brown and a
//!   swamp black: it absorbs blue hardest and red hardly at all.
//! * **`b_p`, suspended particles**, falls off as the inverse of the
//!   wavelength, the power law bio-optical models commonly assume for it.
//!
//! What varies between the presets is the two terms a body of water gains past
//! pure water, [`Preset::dissolved`] and [`Preset::particles`], and those are
//! **chosen, not measured**: a filtered pool carries neither, a clear lake a
//! trace of both, a pond a lot of both, and a blackwater swamp a great deal of
//! dissolved tannin and little silt — which is why it reads dark rather than
//! milky. `the_presets_are_the_model_at_their_two_terms` holds every literal
//! below to the model, so a hand-edited coefficient fails rather than drifting.

use crcbl::render::Medium;

/// Pure water's absorption at 650, 550 and 450 nm, in 1/m — Pope and Fry
/// (1997), as the module header cites.
pub const PURE_WATER_ABSORPTION: [f32; 3] = [0.340, 0.0565, 0.0092];

/// The wavelength each channel stands for, in nanometres.
pub const CHANNEL_WAVELENGTHS: [f32; 3] = [650.0, 550.0, 450.0];

/// What the water in a scene is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    /// A filtered, chlorinated pool: pure water and nothing in it.
    ///
    /// The default, because the courtyard is a pool and the goldens are taken
    /// under it.
    #[default]
    ClearPool,
    /// A clear lake: a trace of dissolved organic matter and of silt.
    Lake,
    /// A pond: a lot of both, so the floor fades within a metre or two.
    Pond,
    /// A blackwater swamp: heavy tannin staining and little silt.
    Swamp,
}

impl Preset {
    /// Every preset, in the order `M` and the page button walk them.
    pub const ALL: [Self; 4] = [Self::ClearPool, Self::Lake, Self::Pond, Self::Swamp];

    /// What the panel, the heartbeat and the page call it.
    ///
    /// Hyphenated rather than spaced, so a heartbeat field is one token a gate
    /// can match without knowing where the next field starts.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ClearPool => "clear-pool",
            Self::Lake => "lake",
            Self::Pond => "pond",
            Self::Swamp => "swamp",
        }
    }

    /// Parses a [`Preset::label`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|preset| preset.label() == name)
    }

    /// The next one, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::ClearPool => Self::Lake,
            Self::Lake => Self::Pond,
            Self::Pond => Self::Swamp,
            Self::Swamp => Self::ClearPool,
        }
    }

    /// Dissolved organic matter's absorption at 440 nm, in 1/m — the
    /// `a_g(440)` of the module header's model.
    #[must_use]
    pub const fn dissolved(self) -> f32 {
        match self {
            Self::ClearPool => 0.0,
            Self::Lake => 0.1,
            Self::Pond => 1.5,
            Self::Swamp => 5.0,
        }
    }

    /// Suspended particles' scattering at 550 nm, in 1/m — the `b_p(550)` of
    /// the module header's model.
    #[must_use]
    pub const fn particles(self) -> f32 {
        match self {
            Self::ClearPool => 0.0,
            Self::Lake => 0.2,
            Self::Pond => 0.6,
            Self::Swamp => 0.5,
        }
    }

    /// The medium itself, per linear-RGB channel.
    ///
    /// Literals rather than the model evaluated here, because `exp` is not a
    /// `const fn` and a medium is read every time a body is set. The test named
    /// in the module header is what keeps the two the same numbers.
    #[must_use]
    pub const fn medium(self) -> Medium {
        match self {
            Self::ClearPool => Medium {
                absorption: [0.340, 0.0565, 0.0092],
                scattering: [0.0007147, 0.001471, 0.0035],
            },
            Self::Lake => Medium {
                absorption: [0.3453, 0.07794, 0.09614],
                scattering: [0.1699, 0.2015, 0.2479],
            },
            Self::Pond => Medium {
                absorption: [0.4193, 0.3781, 1.313],
                scattering: [0.5084, 0.6015, 0.7368],
            },
            Self::Swamp => Medium {
                absorption: [0.6043, 1.128, 4.356],
                scattering: [0.4238, 0.5015, 0.6146],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slope of dissolved organic matter's exponential, per nanometre.
    const SLOPE: f64 = 0.014;

    /// Pure water's total scattering at 500 nm, in 1/m: Morel's `β(90°)`
    /// integrated over the sphere with the depolarisation factor.
    fn pure_water_scattering_at_500() -> f64 {
        let beta_90 = 1.38e-4;
        let depolarisation = 0.09;
        beta_90 * 8.0 * std::f64::consts::PI / 3.0 * (2.0 + depolarisation) / (1.0 + depolarisation)
    }

    /// **Every preset is the module header's model at its two terms**, to the
    /// four places the literals are written to.
    ///
    /// The check that a coefficient edited by hand — or a term changed without
    /// its channels — fails here rather than drawing a medium no source backs.
    #[test]
    fn the_presets_are_the_model_at_their_two_terms() {
        let b_500 = pure_water_scattering_at_500();
        assert!(
            (b_500 - 0.00222).abs() < 5e-6,
            "Morel's pure-water scattering integrates to {b_500}, not the 0.00222 the header cites"
        );
        for preset in Preset::ALL {
            let medium = preset.medium();
            for channel in 0..3 {
                let lambda = f64::from(CHANNEL_WAVELENGTHS[channel]);
                let absorption = f64::from(PURE_WATER_ABSORPTION[channel])
                    + f64::from(preset.dissolved()) * (-SLOPE * (lambda - 440.0)).exp();
                let scattering = 0.00222 * (lambda / 500.0).powf(-4.32)
                    + f64::from(preset.particles()) * 550.0 / lambda;
                for (which, written, model) in [
                    ("absorption", medium.absorption[channel], absorption),
                    ("scattering", medium.scattering[channel], scattering),
                ] {
                    // Four significant figures, which is how the literals are
                    // written: a relative error under a part in two thousand.
                    let error = (f64::from(written) - model).abs() / model;
                    assert!(
                        error < 5e-4,
                        "{}'s {which} at {lambda} nm is written {written} and the model gives \
                         {model:.5}",
                        preset.label()
                    );
                }
            }
        }
    }

    /// **The presets are four different waters**, and each is one a body can
    /// carry: finite and non-negative in every channel.
    #[test]
    fn the_presets_are_distinct_and_each_is_a_valid_medium() {
        for (at, preset) in Preset::ALL.iter().enumerate() {
            let medium = preset.medium();
            for value in medium.absorption.iter().chain(&medium.scattering) {
                assert!(
                    value.is_finite() && *value >= 0.0,
                    "{}: {medium:?}",
                    preset.label()
                );
            }
            for other in &Preset::ALL[..at] {
                assert_ne!(
                    other.medium(),
                    medium,
                    "{other:?} and {preset:?} are one water"
                );
            }
            assert_eq!(Preset::from_name(preset.label()), Some(*preset));
        }
        assert_eq!(Preset::from_name("sea"), None);

        let mut preset = Preset::default();
        for _ in 0..Preset::ALL.len() {
            preset = preset.next();
        }
        assert_eq!(
            preset,
            Preset::default(),
            "the cycle must wrap after every preset"
        );
    }
}
