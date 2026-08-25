//! Piecewise-linear curves over a particle's normalised age.
//!
//! `docs/plan/20-particles.md`'s effect assets describe size and colour over
//! lifetime as `Curve([(0.0, 1.0), (1.0, 0.2)])` and
//! `Gradient([(0.0, "#fff4c0"), (1.0, "#ff5a0000")])`, and say that the GPU
//! form of both is "small 1D LUT textures (bake from the RON curve defs)". This
//! is the authored side of that pair: the keyed stops a bake would sample, and
//! the evaluation a CPU step does instead while there is no bake.
//!
//! [`Curve`] and [`Gradient`] are the same [`Ramp`] over different values,
//! because they are the same knowledge — where the stops are and how to read
//! between them — and only the arithmetic at the very bottom differs.

use std::fmt;

use glam::Vec4;

/// A scalar over a particle's normalised age: size, or a speed multiplier.
pub type Curve = Ramp<f32>;

/// A linear-space RGBA colour over a particle's normalised age.
///
/// Linear, not sRGB: the renderer's materials are spelled in linear colour and
/// a gradient authored in hex is converted once, where it is authored, rather
/// than every time it is sampled.
pub type Gradient = Ramp<Vec4>;

/// A value two of a [`Ramp`]'s stops can be read between.
///
/// Implemented for the two things effects ramp — a scalar and a colour — and
/// deliberately not exposed for anything else: it exists so [`Ramp::eval`] is
/// written once, not as an extension point.
pub trait Lerp: Copy {
    /// `self` at `t == 0.0`, `other` at `t == 1.0`, linear between.
    fn lerp(self, other: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for Vec4 {
    fn lerp(self, other: Self, t: f32) -> Self {
        Vec4::lerp(self, other, t)
    }
}

/// Why a stop list could not become a [`Ramp`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RampError {
    /// The stop list was empty, so there is nothing to evaluate to.
    Empty,
    /// A stop's position is not greater than or equal to the one before it.
    ///
    /// The order is the search: [`Ramp::eval`] walks forwards and stops at the
    /// first key past the sample, which finds the wrong span in an unsorted
    /// list rather than failing.
    Unsorted {
        /// The index of the out-of-order stop.
        index: usize,
    },
    /// A stop's position is not a finite number.
    ///
    /// A `NaN` key compares false against everything, so it would make the
    /// search silently skip its span; an infinite one makes the span it starts
    /// infinitely wide.
    KeyNotFinite {
        /// The index of the offending stop.
        index: usize,
    },
}

impl fmt::Display for RampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Empty => write!(f, "a ramp needs at least one stop"),
            Self::Unsorted { index } => write!(
                f,
                "stop {index} is positioned before the stop that precedes it"
            ),
            Self::KeyNotFinite { index } => {
                write!(f, "stop {index} is positioned at a non-finite key")
            }
        }
    }
}

impl std::error::Error for RampError {}

/// Keyed stops with linear interpolation between them, clamped at both ends.
///
/// # What clamping at the ends buys
///
/// [`eval`](Self::eval) holds the first stop's value below the first key and
/// the last stop's above the last, so a ramp is total: every `t` has an answer
/// and a caller never has to bound one. That matters because the sampler's
/// argument is a particle's age over its lifetime, which reaches exactly `1.0`
/// on the step a particle retires and can overshoot it by a fraction of a step
/// before the retirement is noticed.
///
/// # Cost
///
/// A forward scan of the keys. Effect ramps are the handful of stops an author
/// draws, so the scan is shorter than the branch a binary search would take —
/// and the destination for this data is a baked LUT texture, where the search
/// disappears entirely into a texture fetch.
#[derive(Clone, Debug, PartialEq)]
pub struct Ramp<T> {
    stops: Vec<(f32, T)>,
}

impl<T: Lerp> Ramp<T> {
    /// A ramp from keyed stops, in ascending key order.
    ///
    /// # Errors
    ///
    /// [`RampError`] if the list is empty, if a key is not finite, or if the
    /// keys do not ascend.
    pub fn new(stops: Vec<(f32, T)>) -> Result<Self, RampError> {
        let Some(&(first, _)) = stops.first() else {
            return Err(RampError::Empty);
        };
        if !first.is_finite() {
            return Err(RampError::KeyNotFinite { index: 0 });
        }
        for index in 1..stops.len() {
            let key = stops[index].0;
            if !key.is_finite() {
                return Err(RampError::KeyNotFinite { index });
            }
            if key < stops[index - 1].0 {
                return Err(RampError::Unsorted { index });
            }
        }
        Ok(Self { stops })
    }

    /// A ramp that holds one value everywhere.
    pub fn constant(value: T) -> Self {
        Self {
            stops: vec![(0.0, value)],
        }
    }

    /// The value at `t`, clamped to the end stops outside the keyed span.
    pub fn eval(&self, t: f32) -> T {
        let stops = &self.stops;
        // `new` refuses an empty list, so both ends exist.
        let (first_key, first_value) = stops[0];
        // `NaN` is named rather than left to the comparisons: every one of them
        // is false for it, so an unguarded sample would fall through the loop
        // and return the *last* stop, which is the opposite of clamping.
        if t.is_nan() || t <= first_key {
            return first_value;
        }
        for window in stops.windows(2) {
            let (lo_key, lo_value) = window[0];
            let (hi_key, hi_value) = window[1];
            // Strictly less than, which is what makes two stops at one key a
            // step: the zero-width span between them can never be selected, and
            // a sample exactly on the shared key falls through to the span
            // *after* it and reads the value the step goes to. It is also what
            // keeps the divisor below positive — an ascending key list has no
            // other way to reach here with `hi_key <= lo_key`.
            if t < hi_key {
                let span = hi_key - lo_key;
                debug_assert!(span > 0.0, "a zero-width span was selected");
                return lo_value.lerp(hi_value, (t - lo_key) / span);
            }
        }
        stops[stops.len() - 1].1
    }

    /// The keyed stops, in the order they were given.
    pub fn stops(&self) -> &[(f32, T)] {
        &self.stops
    }
}
