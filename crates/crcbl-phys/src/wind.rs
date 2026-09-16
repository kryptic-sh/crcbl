//! The seam physics reads moving air through.
//!
//! `docs/plan/56-wind.md`'s decision 6: "`crcbl-phys` defines a [`WindQuery`]
//! trait that `crcbl-wind` implements, the same arrangement as
//! `docs/plan/55-water.md`'s `WaterQuery`, **so physics does not depend on the
//! wind crate**." The arrow runs the other way round from the one a reader
//! expects, and that is the whole point: the crate with the bodies in it must
//! not have to link the crate with the weather in it, or a server simulating a
//! room ends up carrying two authored texture layers to answer a question it
//! never asks.
//!
//! # Why the trait is here and empty of implementations
//!
//! There is no implementation in this crate, and there is not going to be a
//! `StillAir` one either: still air is what every force provider in
//! [`crate::forces`] and [`crate::atmosphere`] already assumes, so a type that
//! answers zero would be a second way of saying what "no wind provider" says.
//!
//! # What reads it
//!
//! Nothing yet. `docs/plan/56-wind.md`'s rung W4 is the rigid-body wind drag —
//! quadratic drag on `v_rel = w − v` with a per-body drag coefficient and
//! projected area, limited to the linearised implicit update so it never
//! reverses the relative velocity — and it needs two `crcbl-phys` prerequisites
//! that do not exist yet: rigid-body rotation, and per-body medium properties.
//! W1 builds the seam and the field; the provider that joins them is W4's.

use glam::DVec3;

/// What the air is doing at a point, right now.
///
/// The one question physics asks the wind. It is a **velocity**, in metres per
/// second, in world space — not a force and not a direction-and-speed pair,
/// because what a drag model needs is `w − v` and every other shape would have
/// the caller rebuild that.
///
/// Implementations are expected to be pure: the same position at the same tick
/// answers the same velocity, bit for bit. `crcbl-wind`'s field is, and its
/// determinism test is what says so.
pub trait WindQuery: std::fmt::Debug {
    /// The wind velocity at a world-space position, in m/s.
    fn wind_at(&self, position: DVec3) -> DVec3;
}
