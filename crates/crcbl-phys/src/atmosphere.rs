//! An exponential atmosphere, and the quadratic drag a body moving through it
//! feels.
//!
//! `docs/plan/05-physics.md`'s L1 line: "atmospheric drag `F = ½ρv²·Cd·A` with
//! exponential density-vs-altitude — terminal velocity **emerges**, not
//! scripted". Both halves are here, and the emergence is the point: nothing in
//! this module knows what a terminal velocity is. It falls out of the drag rising
//! as the square of speed until it balances whatever is pulling the body down,
//! and [`AtmosphericDrag`]'s tests measure it against the closed form rather
//! than against a number this file chose.
//!
//! # Why this is not [`DragForce`](crate::forces::DragForce)
//!
//! That one is linear damping, `F = -k·v`, which is the right model for a body
//! in a fluid at low Reynolds number and the wrong one for a rocket. Their
//! terminal velocities do not even have the same shape: linear drag gives
//! `v = mg/k`, quadratic gives `v = √(2mg / ρ·Cd·A)`. A game that wants a
//! spacecraft to slow down in air and reach a speed a person can recognise
//! needs the second. Both stay, because both are real models and neither is a
//! better version of the other.
//!
//! # What drove it
//!
//! `docs/plan/sample/06-orbit.md` — the atmosphere has to be *felt* on the way
//! up and on the way down, and its exit criteria ask for terminal velocity
//! within 1% of the formula for a given `Cd`, `A` and `ρ`. It is also what tells
//! an on-rails propagator that it has to hand back to live integration: above
//! [`Atmosphere::ceiling`] there is nothing to integrate.

use glam::DVec3;

use crate::components::{RigidBody, Transform};
use crate::forces::ForceProvider;

/// An exponential atmosphere around a spherical body.
///
/// Density falls as `ρ(h) = ρ₀ · exp(-h / H)`, where `h` is altitude above the
/// surface and `H` is the scale height — the altitude gain that divides density
/// by `e`. It is the standard first model of an atmosphere in hydrostatic
/// equilibrium at constant temperature, and it is what the plan asks for.
///
/// **It is not a standard-atmosphere table.** A real atmosphere is not
/// isothermal, so this diverges from ISA in the stratosphere; the plan wants
/// drag a player can feel and a terminal velocity that emerges, not an
/// aerodynamics reference. Where a number below names a real one, it says so
/// and says where it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Atmosphere {
    /// Density at the surface, in kg/m³.
    pub sea_level_density: f64,
    /// The altitude gain that divides density by `e`, in metres.
    pub scale_height: f64,
    /// Altitude above the surface at which the atmosphere is treated as gone,
    /// in metres.
    ///
    /// **A hard edge on purpose.** An exponential never reaches zero, so
    /// without this every orbit at every altitude carries a little drag for
    /// ever — which is both wrong to feel and fatal to an on-rails propagator,
    /// because a conic that is being perturbed is not a conic. A body above
    /// this is in vacuum and can be propagated analytically; a body below it
    /// has to be integrated. That is the handoff `06-orbit.md` calls the
    /// bubble, and this is the boundary it is drawn on.
    pub ceiling: f64,
}

impl Atmosphere {
    /// An Earth-like atmosphere.
    ///
    /// `sea_level_density` is 1.225 kg/m³, the International Standard
    /// Atmosphere's sea-level value (ISO 2533, 15 °C at 101.325 kPa).
    ///
    /// `scale_height` is 8500 m. **This one is a fit, not a defined constant**:
    /// ISA is not isothermal and so has no single scale height, and 8.5 km is
    /// the value that matches its density through the troposphere, where
    /// anything flying feels drag. Quoting it as though it came out of the
    /// standard would be citing a number the standard does not contain.
    ///
    /// `ceiling` is 100 km — the Kármán line, the conventional edge of space
    /// (FAI). At that altitude this model already reports about 8.6 × 10⁻⁶
    /// kg/m³, seven parts in a million of the surface value, so the truncation
    /// removes nothing a body could feel.
    pub const EARTH: Self = Self {
        sea_level_density: 1.225,
        scale_height: 8_500.0,
        ceiling: 100_000.0,
    };

    /// Builds an atmosphere.
    ///
    /// # Panics
    ///
    /// If `sea_level_density` is negative, or `scale_height` is not positive.
    /// Both are caller bugs that would otherwise surface as a body accelerating
    /// into the ground or a non-finite density: a zero scale height divides by
    /// zero, and a negative density pushes rather than resists.
    #[must_use]
    pub fn new(sea_level_density: f64, scale_height: f64, ceiling: f64) -> Self {
        assert!(
            sea_level_density >= 0.0,
            "sea-level density must not be negative, got {sea_level_density}"
        );
        assert!(
            scale_height > 0.0,
            "scale height must be positive, got {scale_height}"
        );
        Self {
            sea_level_density,
            scale_height,
            ceiling,
        }
    }

    /// Density at `altitude` metres above the surface, in kg/m³.
    ///
    /// Zero at or above [`ceiling`](Self::ceiling), and clamped to the
    /// surface value below it: a body under the surface is inside the planet,
    /// where this model has nothing to say, and letting the exponential run
    /// away there would apply an enormous force to anything that tunnelled.
    #[must_use]
    pub fn density_at(&self, altitude: f64) -> f64 {
        // A NaN altitude is sent to vacuum rather than into `exp`, which would
        // hand back a NaN density and poison the body's velocity for the rest
        // of the run. An infinite one needs no special case: `-inf` is
        // underground and clamps below, `+inf` is above any finite ceiling.
        if altitude.is_nan() || altitude >= self.ceiling {
            return 0.0;
        }
        if altitude <= 0.0 {
            return self.sea_level_density;
        }
        self.sea_level_density * (-altitude / self.scale_height).exp()
    }
}

/// Quadratic drag through an [`Atmosphere`] around a spherical body.
///
/// `F = -½ · ρ(h) · |v|² · Cd · A · v̂`, opposing the body's motion, with `ρ`
/// read at the body's current altitude. Terminal velocity is not written
/// anywhere: it is where this balances whatever else is accelerating the body.
///
/// **The planet is part of the provider, not of the world.** A
/// [`ForceProvider`] is applied to every dynamic body, and altitude only means
/// something relative to a particular surface — so the centre and radius live
/// here, and a scene with two bodies that have atmospheres has two of these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AtmosphericDrag {
    /// The air this drag is felt in.
    pub atmosphere: Atmosphere,
    /// The centre of the body the atmosphere surrounds, in world space.
    pub centre: DVec3,
    /// The surface radius of that body, in metres. Altitude is measured from
    /// it.
    pub radius: f64,
    /// The dimensionless drag coefficient `Cd`. About 0.5 for a sphere, 0.8 for
    /// a blunt capsule, 0.05 for a streamlined body.
    pub drag_coefficient: f64,
    /// The reference area `A` presented to the flow, in m².
    pub reference_area: f64,
}

impl AtmosphericDrag {
    /// The altitude of a world-space `position` above this body's surface, in
    /// metres. Negative underground.
    #[must_use]
    pub fn altitude_of(&self, position: DVec3) -> f64 {
        (position - self.centre).length() - self.radius
    }

    /// The world-space force this would apply to a body at `position` moving at
    /// `velocity`.
    ///
    /// Public for the reason [`crate::forces::DragForce::world_force`] is: a
    /// provider is global, so a game that drags one entity among a field of
    /// others cannot use the pipeline and would otherwise write `½ρv²CdA` out
    /// again — and a second copy of a formula is where the two drift.
    #[must_use]
    pub fn world_force(&self, position: DVec3, velocity: DVec3) -> DVec3 {
        let speed_squared = velocity.length_squared();
        // Below this there is no direction to oppose: `normalize` on a zero
        // vector is NaN, and a body at rest feels no drag anyway. Compared on
        // the square so no root is taken to decide whether to take one.
        if speed_squared <= f64::MIN_POSITIVE {
            return DVec3::ZERO;
        }
        let density = self.atmosphere.density_at(self.altitude_of(position));
        if density <= 0.0 {
            return DVec3::ZERO;
        }
        // ½ρv²·Cd·A along the direction of travel, opposed. `v² · v̂` is
        // `|v| · v`, which is one root rather than a root and a division.
        let magnitude = 0.5 * density * self.drag_coefficient * self.reference_area;
        -velocity * (magnitude * speed_squared.sqrt())
    }
}

impl ForceProvider for AtmosphericDrag {
    fn apply(&self, body: &mut RigidBody, transform: &Transform, _dt: f64) {
        if body.is_dynamic() {
            let force = self.world_force(transform.position, body.velocity);
            body.apply_force(force);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forces::GravityForce;
    use crate::integrator::{Integrator, SemiImplicitEuler};
    use glam::DQuat;

    /// A planet-sized sphere at the origin, so altitude is just the radius
    /// difference and a test can place a body by adding to it.
    const RADIUS: f64 = 6_371_000.0;

    fn at_altitude(altitude: f64) -> DVec3 {
        DVec3::new(0.0, RADIUS + altitude, 0.0)
    }

    fn drag_with(
        atmosphere: Atmosphere,
        drag_coefficient: f64,
        reference_area: f64,
    ) -> AtmosphericDrag {
        AtmosphericDrag {
            atmosphere,
            centre: DVec3::ZERO,
            radius: RADIUS,
            drag_coefficient,
            reference_area,
        }
    }

    /// **The density curve is the one the plan names**, `ρ₀·exp(-h/H)`,
    /// checked against the formula rather than against remembered values.
    #[test]
    fn density_falls_off_exponentially_with_altitude() {
        let air = Atmosphere::EARTH;
        assert_eq!(
            air.density_at(0.0),
            air.sea_level_density,
            "the surface is the sea-level value by definition"
        );

        // One scale height up is exactly one factor of e down. That is what a
        // scale height *is*, so it is the check that says this is an
        // exponential atmosphere rather than some other decreasing curve.
        let one_up = air.density_at(air.scale_height);
        let expected = air.sea_level_density / std::f64::consts::E;
        assert!(
            (one_up - expected).abs() < expected * 1e-12,
            "one scale height up must divide density by e: got {one_up}, expected {expected}"
        );

        // And it keeps doing it, which a single point could not distinguish
        // from a straight line through that point.
        let two_up = air.density_at(2.0 * air.scale_height);
        let expected_two = air.sea_level_density / (std::f64::consts::E * std::f64::consts::E);
        assert!(
            (two_up - expected_two).abs() < expected_two * 1e-12,
            "two scale heights up must divide it by e twice: got {two_up}, expected {expected_two}"
        );
    }

    /// Vacuum above the ceiling, and no NaN can get through it.
    #[test]
    fn there_is_no_air_above_the_ceiling_and_none_at_a_nan_altitude() {
        let air = Atmosphere::EARTH;
        assert_eq!(
            air.density_at(air.ceiling),
            0.0,
            "the ceiling itself is vacuum"
        );
        assert_eq!(air.density_at(air.ceiling * 2.0), 0.0);
        assert_eq!(
            air.density_at(f64::NAN),
            0.0,
            "a NaN altitude must land in vacuum rather than being multiplied into a force"
        );
        // Underground is the surface value rather than a runaway exponential.
        assert_eq!(
            air.density_at(-1_000.0),
            air.sea_level_density,
            "below the surface must clamp to sea level, not run the exponential away"
        );
    }

    /// Drag opposes motion, and a body at rest feels none — including no NaN
    /// from normalising a zero vector.
    #[test]
    fn drag_opposes_motion_and_a_still_body_feels_none() {
        let drag = drag_with(Atmosphere::EARTH, 0.5, 1.0);
        let position = at_altitude(0.0);

        let velocity = DVec3::new(30.0, -40.0, 0.0);
        let force = drag.world_force(position, velocity);
        assert!(
            force.dot(velocity) < 0.0,
            "drag must oppose the direction of travel: {force:?} against {velocity:?}"
        );
        assert!(force.is_finite(), "{force:?}");

        assert_eq!(
            drag.world_force(position, DVec3::ZERO),
            DVec3::ZERO,
            "a body at rest has no direction to be slowed along"
        );
    }

    /// **The exit criterion from `docs/plan/sample/06-orbit.md`, measured:**
    /// terminal velocity within 1% of `√(2mg / ρ·Cd·A)`.
    ///
    /// Nothing in this module knows that formula. The body is dropped with
    /// gravity and this drag, integrated until it stops accelerating, and what
    /// it settles at is compared against the closed form — so the test fails if
    /// the drag law is wrong in *any* of its factors, and passes only because
    /// `½ρv²CdA` is what was written.
    ///
    /// **The atmosphere here is deliberately uniform** — a scale height so large
    /// that density does not measurably change over the kilometres the body
    /// falls. The closed form assumes a constant `ρ`, so a real profile would
    /// mean comparing against a number that moved while the body fell, and the
    /// profile has its own test above. This one is about the drag law.
    #[test]
    fn terminal_velocity_emerges_within_one_percent_of_the_closed_form() {
        const MASS: f64 = 100.0;
        const CD: f64 = 0.5;
        const AREA: f64 = 1.0;
        const G: f64 = 9.81;
        const DT: f64 = 1.0 / 240.0;

        let uniform = Atmosphere::new(1.225, 1.0e12, f64::INFINITY);
        let drag = drag_with(uniform, CD, AREA);
        let gravity = GravityForce::new(DVec3::new(0.0, -G, 0.0));
        let integrator = SemiImplicitEuler;

        let mut body = RigidBody::new_dynamic(MASS);
        let mut transform = Transform {
            position: at_altitude(10_000.0),
            rotation: DQuat::IDENTITY,
        };

        // Sixty seconds, against a time constant of `v_term / g` — about six
        // seconds here — so this is ten of them and the approach has converged
        // long before the end.
        for _ in 0..(60.0 / DT) as u32 {
            gravity.apply(&mut body, &transform, DT);
            drag.apply(&mut body, &transform, DT);
            integrator.step(&mut body, &mut transform, DT);
        }

        let settled = body.velocity.length();
        let density = uniform.density_at(drag.altitude_of(transform.position));
        let expected = (2.0 * MASS * G / (density * CD * AREA)).sqrt();
        assert!(
            (settled - expected).abs() <= expected * 0.01,
            "terminal velocity {settled} m/s is not within 1% of the closed form \
             {expected} m/s for m={MASS} g={G} rho={density} Cd={CD} A={AREA}"
        );
    }

    /// Thinner air, faster fall — the half of the model a uniform atmosphere
    /// cannot show.
    ///
    /// It is the ordering that is asserted rather than either number, because
    /// the numbers are the previous test's business and this one is about the
    /// coupling: a drag that ignored altitude would give the same speed at both.
    #[test]
    fn the_same_body_falls_faster_where_the_air_is_thinner() {
        let drag = drag_with(Atmosphere::EARTH, 0.5, 1.0);
        let velocity = DVec3::new(0.0, -100.0, 0.0);

        let low = drag.world_force(at_altitude(1_000.0), velocity).length();
        let high = drag.world_force(at_altitude(30_000.0), velocity).length();
        assert!(
            high < low,
            "drag at 30 km ({high} N) must be less than at 1 km ({low} N)"
        );
        assert!(low > 0.0, "there must be drag at 1 km at all");
    }
}
