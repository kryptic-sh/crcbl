//! The world's one gust scroll offset, accumulated in integer fixed point.
//!
//! `docs/plan/56-wind.md`'s decision 3, and the trap it is written around.
//! *God of War*'s vegetation talk records that **each object scrolling its own
//! offset diverges from its neighbours as soon as direction or speed changes**,
//! and that repairing it afterwards cost flow-map flips with log-binned speeds.
//! So there is one offset for the world, `o ← o + direction · gust speed · Δt`,
//! and every consumer reads it.
//!
//! # Why integer, and why this many fractional bits
//!
//! A gust offset is the one quantity in the field that is *accumulated* rather
//! than computed, so it is the one that can drift. In `f32` it stops advancing
//! at all once it is large: the gap between neighbouring `f32`s grows with the
//! value, and once it is wider than twice the step every tick rounds back to
//! where it started and the gusts stand still. That is not a hypothetical —
//! `a_far_out_offset_still_advances_by_a_whole_step` runs the `f32` arithmetic
//! beside this type's and watches it happen.
//!
//! Integers do not have that failure. What they have instead is a rounding
//! error on each step, and [`FRACTION_BITS`] is chosen so that error cannot
//! matter: a step is rounded to the nearest 2⁻³² m, about 0.23 nm, so a tick
//! rate of 60 Hz accumulates at most 14 nm of error per second even if every
//! rounding went the same way. The whole range still reaches ±2.1 × 10⁹ m,
//! four times the Earth–Moon distance.

use glam::DVec2;

use crate::weather::Weather;

/// Fractional bits each axis of a [`ScrollOffset`] carries.
pub const FRACTION_BITS: u32 = 32;

/// Fixed-point units in one metre.
pub const UNITS_PER_METRE: f64 = (1u64 << FRACTION_BITS) as f64;

/// The one gust offset for the world, in 2⁻³² m fixed point on the XZ plane.
///
/// Held as units rather than metres so that adding a step is exact. Reading it
/// back as metres ([`Self::metres`]) is a conversion and a division, both of
/// which are exact operations on this representation, but a *sum* of such
/// conversions would not be — which is why nothing accumulates the metres.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ScrollOffset {
    x: i64,
    z: i64,
}

impl ScrollOffset {
    /// No scroll at all — the field's gusts stand still.
    pub const ZERO: Self = Self { x: 0, z: 0 };

    /// An offset from its two axes in fixed-point units.
    #[inline]
    #[must_use]
    pub const fn from_units(x: i64, z: i64) -> Self {
        Self { x, z }
    }

    /// The two axes in fixed-point units.
    #[inline]
    #[must_use]
    pub const fn units(self) -> (i64, i64) {
        (self.x, self.z)
    }

    /// The offset in metres on the XZ plane.
    #[inline]
    #[must_use]
    pub fn metres(self) -> DVec2 {
        DVec2::new(self.x as f64, self.z as f64) / UNITS_PER_METRE
    }

    /// What one tick of `dt` seconds under `weather` adds, in units.
    ///
    /// Separated from [`Self::advance`] because it is the quantity a test has
    /// to be able to name: a gust travelling a known distance takes a whole
    /// number of these, and a distance picked any other way is one the fixed
    /// point cannot land on exactly.
    ///
    /// A non-finite or absurd product becomes a saturated step rather than a
    /// panic — `as` on a float saturates and maps `NaN` to zero — because a
    /// field whose weather went wrong should stop scrolling, not stop the tick.
    #[must_use]
    pub fn step(weather: &Weather, dt: f64) -> (i64, i64) {
        let travel = weather.direction() * weather.gust_speed * dt * UNITS_PER_METRE;
        (travel.x.round() as i64, travel.y.round() as i64)
    }

    /// Advances the offset by one tick of `dt` seconds under `weather`.
    ///
    /// Saturating rather than wrapping: an offset that reached ±2.1 × 10⁹ m is
    /// a session that ran for seventy years at a metre a second, and stopping
    /// there is a field that has gone still rather than one that jumps to the
    /// other end of the world.
    pub fn advance(&mut self, weather: &Weather, dt: f64) {
        let (x, z) = Self::step(weather, dt);
        self.x = self.x.saturating_add(x);
        self.z = self.z.saturating_add(z);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breezy_east() -> Weather {
        Weather::new(DVec2::X, 4.0).expect("a real direction")
    }

    #[test]
    fn a_step_is_the_distance_the_gust_travels() {
        let weather = breezy_east();
        let (x, z) = ScrollOffset::step(&weather, 0.25);
        assert_eq!(
            x,
            (1.0 * UNITS_PER_METRE) as i64,
            "4 m/s for a quarter second"
        );
        assert_eq!(z, 0);
    }

    #[test]
    fn a_thousand_ticks_land_exactly_a_thousand_steps_on() {
        let weather = breezy_east();
        let dt = 1.0 / 60.0;
        let (step_x, step_z) = ScrollOffset::step(&weather, dt);
        let mut offset = ScrollOffset::ZERO;
        for _ in 0..1000 {
            offset.advance(&weather, dt);
        }
        assert_eq!(offset.units(), (step_x * 1000, step_z * 1000));
    }

    #[test]
    fn a_far_out_offset_still_advances_by_a_whole_step() {
        // The failure an `f32` offset has: at this distance an `f32` cannot
        // represent a 17 mm step at all, and the offset stops moving.
        let weather = breezy_east();
        let dt = 1.0 / 60.0;
        let (step_x, _) = ScrollOffset::step(&weather, dt);
        // Ten million metres: an `f32` there has a step of one metre, so the
        // 67 mm this tick asks for rounds away entirely.
        let far = 10_000_000.0;
        let mut offset = ScrollOffset::from_units((far * UNITS_PER_METRE) as i64, 0);
        let before = offset.units().0;
        offset.advance(&weather, dt);
        assert_eq!(offset.units().0 - before, step_x);

        let as_f32 = far as f32;
        assert_eq!(
            as_f32 + (dt as f32) * 4.0,
            as_f32,
            "the same step in `f32` is what this representation exists to avoid"
        );
    }

    #[test]
    fn a_still_field_does_not_scroll() {
        let mut weather = breezy_east();
        weather.gust_speed = 0.0;
        let mut offset = ScrollOffset::from_units(7, -3);
        offset.advance(&weather, 1.0);
        assert_eq!(offset.units(), (7, -3));
    }

    #[test]
    fn the_offset_saturates_rather_than_wrapping() {
        let weather = breezy_east();
        let mut offset = ScrollOffset::from_units(i64::MAX - 2, 0);
        offset.advance(&weather, 1.0);
        assert_eq!(offset.units().0, i64::MAX);
    }

    #[test]
    fn metres_reads_back_what_units_hold() {
        let offset = ScrollOffset::from_units((2.5 * UNITS_PER_METRE) as i64, -(1 << 31));
        assert_eq!(offset.metres(), DVec2::new(2.5, -0.5));
    }
}
