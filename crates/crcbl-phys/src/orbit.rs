//! Analytic two-body propagation: where a coasting body is at time `t`,
//! without integrating anything to find out.
//!
//! `docs/plan/05-physics.md`: "distant bodies = analytic Kepler orbits
//! (`position = f(t)`, zero integration cost, stable forever). Live
//! integration only inside **bubbles** around observers."
//!
//! # Why not just integrate
//!
//! A symplectic integrator bounds energy error but not phase: run one for ten
//! thousand orbits and the body is on the right ellipse at the wrong place on
//! it. Timewarp makes that the normal case rather than the extreme one — at
//! ×1000 a minute of play is most of a day — and doing it by integrating means
//! a thousand times the substeps for the same second of wall clock, which is
//! the cost the rails exist to avoid. An analytic solution has neither problem:
//! it costs the same for `t + 1 s` as for `t + 1000 years`, and it does not
//! drift, because there is nothing accumulating.
//!
//! # Universal variables
//!
//! The propagator is the universal-variable formulation (Bate, Mueller & White,
//! *Fundamentals of Astrodynamics*, §4.4–4.5; Vallado, *Fundamentals of
//! Astrodynamics and Applications*, algorithm `KEPLER`). It advances a state
//! vector directly, so **one code path covers every conic** — a circular
//! parking orbit, a transfer ellipse, an escape hyperbola — and none of the
//! angles the classical elements are built from is ever formed: no ascending
//! node, no argument of periapsis, no anomaly.
//!
//! That last part is the point. The classical elements are singular exactly
//! where a game spends its time: the ascending node is undefined for an
//! equatorial orbit and the argument of periapsis for a circular one, so a
//! propagator built on them is at its worst around the parking orbit every
//! mission starts in. [`Orbit`] reports only the quantities that stay defined
//! there — size, shape, periapsis, apoapsis, period.
//!
//! # What drove it
//!
//! `docs/plan/sample/06-orbit.md` milestone 2, "stable orbit + timewarp", and
//! its exit criterion of an orbital period within 0.1% of the real one.

use crcbl_core::WorldPos;

use crate::frames::State;

/// How many Newton steps the universal-variable solve may take before the
/// bracket it is safeguarded by has to finish the job on its own.
///
/// Newton doubles its correct digits per step from these initial guesses, so a
/// converged solve uses a handful; the cap is not a budget but a guard against
/// a cycle, and reaching it is not a failure because each step that leaves the
/// bracket is replaced by a bisection that halves it.
const MAX_NEWTON_STEPS: u32 = 64;

/// Convergence threshold on the universal anomaly, relative to its own size.
///
/// Near the `f64` floor: the anomaly is `sqrt(a) * E` for an ellipse, so this
/// is a fraction of a millimetre-second on any orbit a game holds.
const CHI_TOLERANCE: f64 = 1.0e-12;

/// The size and shape of a two-body orbit, in the frame of the body it is
/// about.
///
/// Only the quantities that are defined for every orbit: a circular orbit has
/// no argument of periapsis and an equatorial one no ascending node, and a
/// flight UI that displayed either would be showing noise. What it needs —
/// how high, how eccentric, how long — is all here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    /// Semi-major axis in metres: positive for an ellipse, negative for a
    /// hyperbola, and infinite for the parabolic knife edge between them.
    pub semi_major_axis: f64,
    /// Eccentricity: 0 circular, below 1 elliptic, 1 parabolic, above 1
    /// hyperbolic.
    pub eccentricity: f64,
    /// Semi-latus rectum in metres — the orbit's width at the focus.
    ///
    /// Carried because it is the one size that stays finite across the
    /// parabolic case, where the semi-major axis does not, and because both
    /// apsides come from it.
    pub semi_latus_rectum: f64,
    /// Specific orbital energy in J/kg: `v²/2 - mu/r`, negative for a bound
    /// orbit.
    ///
    /// The invariant to watch. Nothing this module does may change it, so a
    /// test that propagates and compares it is asking whether the analytic
    /// solution really is one.
    pub specific_energy: f64,
}

impl Orbit {
    /// The orbit a body at `state` is on about a body of gravitational
    /// parameter `mu`.
    ///
    /// # Panics
    ///
    /// If `mu` is not positive, or the state sits exactly on the centre of
    /// attraction, where there is no orbit and every element divides by zero.
    #[must_use]
    pub fn from_state(mu: f64, state: State) -> Self {
        assert!(mu > 0.0, "a gravitational parameter is positive, got {mu}");
        let position = state.position.delta(WorldPos::ORIGIN);
        let velocity = state.velocity;
        let radius = position.length();
        assert!(
            radius > 0.0,
            "a body at the centre of attraction is not on an orbit"
        );

        let speed_squared = velocity.length_squared();
        let specific_energy = 0.5 * speed_squared - mu / radius;
        // Infinite on the parabolic edge rather than enormous, which is the
        // honest answer and what `period` and `apoapsis` test. Taken from
        // `inverse_semi_major_axis` rather than from `-mu / 2E`, which is the
        // same quantity algebraically and not the same `f64`: the propagator
        // folds whole revolutions using the first, so a period derived from the
        // second would leave a full revolution a few picoseconds short of one.
        let semi_major_axis = 1.0 / inverse_semi_major_axis(mu, radius, speed_squared);

        // The eccentricity vector points at periapsis and has the eccentricity
        // for its length. Formed from the state rather than from angles, so it
        // is as defined for a circular orbit as for any other — it just comes
        // out near zero, and nothing divides by it.
        let eccentricity_vector =
            ((speed_squared - mu / radius) * position - position.dot(velocity) * velocity) / mu;
        let eccentricity = eccentricity_vector.length();

        // `h²/mu`, which stays finite where the semi-major axis does not.
        let semi_latus_rectum = position.cross(velocity).length_squared() / mu;

        Self {
            semi_major_axis,
            eccentricity,
            semi_latus_rectum,
            specific_energy,
        }
    }

    /// Whether the orbit closes — an ellipse rather than a parabola or
    /// hyperbola.
    ///
    /// Read off the semi-major axis rather than the sign of the energy. The two
    /// say the same thing about an orbit and can disagree by a rounding on the
    /// parabolic edge, and it is the axis the period and the apoapsis are
    /// computed from — so an orbit that reported itself closed and then had no
    /// period would be the worse answer.
    #[inline]
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.semi_major_axis > 0.0 && self.semi_major_axis.is_finite()
    }

    /// Distance from the centre of attraction at the closest point, in metres.
    ///
    /// From the semi-latus rectum rather than from `a(1 - e)`, so it is finite
    /// for an escape trajectory too.
    #[inline]
    #[must_use]
    pub fn periapsis(&self) -> f64 {
        self.semi_latus_rectum / (1.0 + self.eccentricity)
    }

    /// Distance at the furthest point, or `None` for an orbit that does not
    /// come back.
    #[must_use]
    pub fn apoapsis(&self) -> Option<f64> {
        self.is_closed()
            .then(|| self.semi_latus_rectum / (1.0 - self.eccentricity))
    }

    /// The orbital period in seconds, or `None` for an orbit that does not
    /// come back.
    ///
    /// Kepler's third law, `T = 2π·sqrt(a³/mu)`.
    #[must_use]
    pub fn period(&self, mu: f64) -> Option<f64> {
        self.is_closed().then(|| {
            std::f64::consts::TAU
                * (self.semi_major_axis * self.semi_major_axis * self.semi_major_axis / mu).sqrt()
        })
    }
}

/// Where a body at `state` will be `dt` seconds later, coasting under the
/// gravity of a body of gravitational parameter `mu` at the frame's origin.
///
/// `dt` may be negative, which propagates backwards. The cost does not depend
/// on its size: a century is the same handful of Newton steps as a second.
///
/// The state is read in whatever frame it was given in, and comes back in the
/// same one. The arithmetic is done on the offset from that frame's origin
/// rather than on the sector split: an orbit is bounded by its frame's sphere
/// of influence, where `f64` metres are good to a fraction of a micrometre.
/// The split earns its keep at the frame conversion, not here.
///
/// Only gravity acts — a burn or an atmosphere makes the trajectory
/// something other than a conic, which is why `06-orbit.md` drops out of
/// timewarp for both.
///
/// # Panics
///
/// If `mu` is not positive, if `dt` is not finite, or if the state sits on the
/// centre of attraction.
#[must_use]
pub fn propagate(mu: f64, state: State, dt: f64) -> State {
    assert!(mu > 0.0, "a gravitational parameter is positive, got {mu}");
    assert!(dt.is_finite(), "cannot propagate by {dt} seconds");

    let position = state.position.delta(WorldPos::ORIGIN);
    let velocity = state.velocity;
    let radius = position.length();
    assert!(
        radius > 0.0,
        "a body at the centre of attraction is not on an orbit"
    );

    // `alpha` is `1/a`: positive elliptic, zero parabolic, negative hyperbolic.
    // Formed this way rather than as `1.0 / a` so the parabolic case is a plain
    // zero instead of a division by an infinity.
    let root_mu = mu.sqrt();
    let alpha = inverse_semi_major_axis(mu, radius, velocity.length_squared());
    // `r·v/sqrt(mu)`, which the time-of-flight equation carries throughout.
    let sigma = position.dot(velocity) / root_mu;

    // A closed orbit repeats exactly, so whole revolutions are not something to
    // solve for: fold them away first. Solving for them instead would ask
    // Newton to find a root ten thousand revolutions along a function that has
    // ten thousand nearby ones, which is where a propagator asked to timewarp
    // stops converging.
    //
    // The remainder keeps the sign of `dt` rather than being made positive.
    // Rewriting a second backwards as most of a revolution forwards is exact in
    // theory and ruinous in practice: the period comes from `alpha`, which for
    // a near-parabolic orbit is a difference of two nearly equal numbers and
    // carries a large relative error, and a whole revolution multiplies it.
    let dt = match period_of(mu, alpha) {
        Some(period) => dt % period,
        None => dt,
    };
    if dt == 0.0 {
        return state;
    }

    let chi = solve_universal_anomaly(root_mu, radius, sigma, alpha, dt);

    // Lagrange's f and g: the propagated state is a combination of the two
    // vectors it started with, which is what makes this exact rather than a
    // step.
    let psi = chi * chi * alpha;
    let (c2, c3) = stumpff(psi);
    let f = 1.0 - chi * chi * c2 / radius;
    let g = dt - chi * chi * chi * c3 / root_mu;
    let new_position = f * position + g * velocity;
    let new_radius = new_position.length();
    let f_dot = root_mu * chi * (psi * c3 - 1.0) / (new_radius * radius);
    let g_dot = 1.0 - chi * chi * c2 / new_radius;

    State::new(
        WorldPos::from_offset(new_position),
        f_dot * position + g_dot * velocity,
    )
}

/// `1/a`, the inverse semi-major axis, from the vis-viva relation.
///
/// The inverse rather than the axis itself because it is what the
/// universal-variable solve wants and because it is the form that stays finite
/// on the parabolic edge, where `a` does not: `alpha` is simply zero there.
/// Positive elliptic, negative hyperbolic.
///
/// One function rather than the expression written at each use, because the
/// two forms of it — this and `-mu / 2E` — are equal on paper and differ in the
/// last bits, and a propagator that folds revolutions by one while reporting a
/// period from the other never quite closes an orbit.
fn inverse_semi_major_axis(mu: f64, radius: f64, speed_squared: f64) -> f64 {
    2.0 / radius - speed_squared / mu
}

/// The period of the orbit with inverse semi-major axis `alpha`, or `None` if
/// it does not close.
fn period_of(mu: f64, alpha: f64) -> Option<f64> {
    (alpha > 0.0).then(|| {
        let a = 1.0 / alpha;
        std::f64::consts::TAU * (a * a * a / mu).sqrt()
    })
}

/// Solves Kepler's time-of-flight equation for the universal anomaly `chi`.
///
/// Newton's method safeguarded by a bracket: time of flight rises strictly
/// with `chi`, so a bracket that contains the root keeps containing it, and any
/// Newton step that leaves it is replaced by a bisection. That is the standard
/// answer to Newton's one real failure mode here — a near-parabolic orbit,
/// where the derivative at the initial guess points the step past the root and
/// an unguarded iteration wanders off. It cannot fail to converge: worst case
/// it is bisection, which halves the bracket every step.
fn solve_universal_anomaly(root_mu: f64, radius: f64, sigma: f64, alpha: f64, dt: f64) -> f64 {
    let target = root_mu * dt;
    // Time of flight minus the time asked for, and its derivative, which is the
    // radius at `chi` — always positive, so the function is monotone.
    let residual = |chi: f64| {
        let psi = chi * chi * alpha;
        let (c2, c3) = stumpff(psi);
        let flight =
            chi * chi * chi * c3 + sigma * chi * chi * c2 + radius * chi * (1.0 - psi * c3);
        let slope = chi * chi * c2 + sigma * chi * (1.0 - psi * c3) + radius * (1.0 - psi * c2);
        (flight - target, slope)
    };

    let (mut low, mut high) = bracket(&residual, alpha, dt);
    // Vallado's opening guess for a closed orbit, `sqrt(mu)·dt/a`, which is
    // exact for a circle and close for anything nearly one — the orbits a game
    // spends its time in. Clamped into the bracket so a poor guess on an
    // eccentric orbit costs a bisection rather than the guarantee.
    let mut chi = if alpha > 0.0 {
        (root_mu * dt * alpha).clamp(low, high)
    } else {
        0.5 * (low + high)
    };
    for _ in 0..MAX_NEWTON_STEPS {
        let (value, slope) = residual(chi);
        if value > 0.0 {
            high = chi
        } else {
            low = chi
        }

        let step = if slope > 0.0 {
            value / slope
        } else {
            f64::INFINITY
        };
        let next = chi - step;
        // Outside the bracket, or a step that is not a number: bisect instead.
        let next = if next > low && next < high {
            next
        } else {
            0.5 * (low + high)
        };

        let moved = (next - chi).abs();
        chi = next;
        if moved <= CHI_TOLERANCE * chi.abs().max(1.0) {
            break;
        }
    }
    chi
}

/// A bracket `[low, high]` around the universal anomaly for `dt`.
///
/// For a closed orbit the answer is exact and free: `chi` is `sqrt(a)` times
/// the eccentric anomaly, which runs over one revolution, and `dt` has already
/// been folded into one. Otherwise the upper end is doubled until the residual
/// changes sign, which terminates because time of flight grows without bound.
fn bracket(residual: &impl Fn(f64) -> (f64, f64), alpha: f64, dt: f64) -> (f64, f64) {
    if alpha > 0.0 {
        // One revolution, on whichever side of the start the time asked for
        // lies: `chi` is `sqrt(a)` times the eccentric anomaly and `dt` has
        // already been folded into a single revolution.
        let revolution = std::f64::consts::TAU / alpha.sqrt();
        return if dt < 0.0 {
            (-revolution, 0.0)
        } else {
            (0.0, revolution)
        };
    }
    // Open orbit: `dt` was not folded, so it can be negative, and `chi` takes
    // the sign of the time.
    let mut edge = dt.signum();
    // The residual at zero has the opposite sign to the one being hunted, so
    // the loop stops as soon as `edge` reaches past the root.
    while residual(edge).0.signum() == residual(0.0).0.signum() {
        edge *= 2.0;
        assert!(
            edge.is_finite(),
            "no universal anomaly brackets a flight time of {dt} seconds"
        );
    }
    if edge < 0.0 { (edge, 0.0) } else { (0.0, edge) }
}

/// The Stumpff functions `C(psi)` and `S(psi)`.
///
/// They are what makes one formula cover every conic: circular functions for a
/// bound orbit, hyperbolic ones for an escape, and the same limit for both as
/// the orbit approaches parabolic.
///
/// Near zero both expressions are a small difference of two nearly equal
/// numbers divided by a small number, which loses most of the mantissa, so the
/// series each converges to is used instead. The threshold is where the two
/// agree to better than `f64` can tell them apart.
fn stumpff(psi: f64) -> (f64, f64) {
    /// Below this the closed forms cancel catastrophically and the series is
    /// both faster and more accurate.
    const SERIES_BELOW: f64 = 1.0e-6;

    if psi > SERIES_BELOW {
        let root = psi.sqrt();
        ((1.0 - root.cos()) / psi, (root - root.sin()) / (root * psi))
    } else if psi < -SERIES_BELOW {
        let root = (-psi).sqrt();
        (
            (root.cosh() - 1.0) / -psi,
            (root.sinh() - root) / (root * -psi),
        )
    } else {
        // The first three terms of each series. At `|psi| <= 1e-6` the next
        // term is below `1e-14` of the leading one, under the `f64` epsilon
        // of the result.
        (
            0.5 - psi / 24.0 + psi * psi / 720.0,
            1.0 / 6.0 - psi / 120.0 + psi * psi / 5040.0,
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    /// Earth's gravitational parameter, JPL DE440 (`GM`, m³/s²).
    const EARTH_MU: f64 = 3.986_004_418e14;

    /// A circular orbit of the given radius, in the plane, going anticlockwise.
    fn circular(radius: f64) -> State {
        State::new(
            WorldPos::from_offset(DVec3::new(radius, 0.0, 0.0)),
            DVec3::new(0.0, (EARTH_MU / radius).sqrt(), 0.0),
        )
    }

    /// An orbit at periapsis `low` with apoapsis `high`, in the plane.
    fn transfer(low: f64, high: f64) -> State {
        let a = 0.5 * (low + high);
        State::new(
            WorldPos::from_offset(DVec3::new(low, 0.0, 0.0)),
            DVec3::new(0.0, (EARTH_MU * (2.0 / low - 1.0 / a)).sqrt(), 0.0),
        )
    }

    fn radius_of(state: State) -> f64 {
        state.position.delta(WorldPos::ORIGIN).length()
    }

    /// **The exit criterion's period check**, against a number that exists
    /// outside this file: a satellite at the geostationary radius takes one
    /// sidereal day to go round.
    ///
    /// The radius is 42 164.17 km and the sidereal day 86 164.0905 s (IERS);
    /// `06-orbit.md` asks for 0.1%, which is far looser than what this
    /// actually manages — but it is the bar the plan set, and a tighter one
    /// here would be asserting on the inputs' own precision rather than on the
    /// formula.
    #[test]
    fn the_period_of_a_geostationary_orbit_is_a_sidereal_day() {
        const SIDEREAL_DAY: f64 = 86_164.090_5;
        let orbit = Orbit::from_state(EARTH_MU, circular(42_164_170.0));
        let period = orbit.period(EARTH_MU).expect("a circular orbit closes");
        assert!(
            (period - SIDEREAL_DAY).abs() <= SIDEREAL_DAY * 0.001,
            "the geostationary period came out {period} s against a sidereal day of {SIDEREAL_DAY} s"
        );
    }

    /// A circle is the case the classical elements cannot describe — no
    /// periapsis to measure an argument from — and the one a mission spends
    /// most of its time in. It propagates like any other.
    #[test]
    fn a_circular_orbit_comes_a_quarter_turn_in_a_quarter_period() {
        const RADIUS: f64 = 7_000_000.0;
        let start = circular(RADIUS);
        let orbit = Orbit::from_state(EARTH_MU, start);
        assert!(
            orbit.eccentricity < 1.0e-12,
            "a circular orbit has no eccentricity to speak of, and nothing may \
             divide by it, but it came out {}",
            orbit.eccentricity
        );
        let period = orbit.period(EARTH_MU).expect("a circular orbit closes");

        let quarter = propagate(EARTH_MU, start, period / 4.0);
        let position = quarter.position.delta(WorldPos::ORIGIN);
        // A quarter turn anticlockwise from `+x` is `+y`, at the same radius.
        let expected = DVec3::new(0.0, RADIUS, 0.0);
        assert!(
            (position - expected).length() < 1.0e-3,
            "a quarter period should reach {expected:?}, came out {position:?}"
        );
        assert!(
            (quarter.velocity.length() - start.velocity.length()).abs() < 1.0e-6,
            "speed is constant on a circle"
        );

        // Half a period puts it on the far side, which the quarter alone could
        // not distinguish from a body that stopped there.
        let half = propagate(EARTH_MU, start, period / 2.0);
        let position = half.position.delta(WorldPos::ORIGIN);
        assert!(
            (position - DVec3::new(-RADIUS, 0.0, 0.0)).length() < 1.0e-3,
            "half a period should reach the far side, came out {position:?}"
        );
    }

    /// The apsides come back as the altitudes the orbit was built from, and
    /// half a period actually arrives at the far one.
    #[test]
    fn a_transfer_orbit_reaches_the_apoapsis_it_reports() {
        const LOW: f64 = 6_571_000.0;
        const HIGH: f64 = 46_371_000.0;
        let start = transfer(LOW, HIGH);
        let orbit = Orbit::from_state(EARTH_MU, start);

        assert!((orbit.periapsis() - LOW).abs() < 1.0e-3, "{:?}", orbit);
        let apoapsis = orbit.apoapsis().expect("a transfer orbit closes");
        assert!((apoapsis - HIGH).abs() < 1.0e-3, "{apoapsis}");
        assert!(
            orbit.eccentricity > 0.0 && orbit.eccentricity < 1.0,
            "a transfer orbit is an ellipse, got e = {}",
            orbit.eccentricity
        );

        let period = orbit.period(EARTH_MU).expect("a transfer orbit closes");
        let arrived = radius_of(propagate(EARTH_MU, start, period / 2.0));
        assert!(
            (arrived - HIGH).abs() < 1.0e-3,
            "half a period should arrive at apoapsis {HIGH} m, came out {arrived} m"
        );

        // **The same orbit read from somewhere that is not an apsis.** Every
        // fixture above starts at periapsis, where the position and the
        // velocity are perpendicular and half the eccentricity vector vanishes
        // — so a term dropped from it would go unnoticed. A third of a period
        // along, nothing is perpendicular to anything.
        let elsewhere = propagate(EARTH_MU, start, period / 3.0);
        let radius = radius_of(elsewhere);
        assert!(
            radius > LOW * 1.01 && radius < HIGH * 0.99,
            "the sample point must be off both apsides, and it is at {radius} m"
        );
        let same = Orbit::from_state(EARTH_MU, elsewhere);
        assert!(
            (same.eccentricity - orbit.eccentricity).abs() < 1.0e-9,
            "read from {radius} m the eccentricity became {} against {}",
            same.eccentricity,
            orbit.eccentricity
        );
        assert!(
            (same.periapsis() - LOW).abs() < 1.0e-3,
            "read from off-apsis the periapsis became {} m",
            same.periapsis()
        );
        assert!(
            (same.apoapsis().expect("still closed") - HIGH).abs() < 1.0e-3,
            "read from off-apsis the apoapsis became {:?} m",
            same.apoapsis()
        );
    }

    /// **`06-orbit.md`'s stability criterion, measured.** Ten thousand
    /// propagations, and the orbit is the orbit it started as.
    ///
    /// Each step feeds the previous result back in, which is the pessimistic
    /// case: a game anchored to a fixed epoch would accumulate nothing at all,
    /// and re-anchoring every step is what a timewarp that keeps re-reading the
    /// live state does. The step is a fraction of a period rather than a whole
    /// one so the fold in [`propagate`] cannot make the test trivial by
    /// returning its input.
    ///
    /// The bounds are two orders of magnitude above what this measured, which
    /// leaves room for a different `libm` on another platform without leaving
    /// room for an actual defect: a propagator that had stopped conserving
    /// anything would be out by kilometres, not micrometres.
    #[test]
    fn ten_thousand_revolutions_leave_the_orbit_where_it_was() {
        const STEPS: u32 = 10_000;
        const STEP_FRACTION: f64 = 0.37;
        const RADIUS: f64 = 42_164_170.0;

        let start = circular(RADIUS);
        let before = Orbit::from_state(EARTH_MU, start);
        let step = before.period(EARTH_MU).expect("a circular orbit closes") * STEP_FRACTION;

        let mut chained = start;
        for _ in 0..STEPS {
            chained = propagate(EARTH_MU, chained, step);
        }
        let after = Orbit::from_state(EARTH_MU, chained);

        let energy_drift =
            (after.specific_energy - before.specific_energy).abs() / before.specific_energy.abs();
        assert!(
            energy_drift < 1.0e-9,
            "specific energy drifted by {energy_drift} of itself over {STEPS} propagations"
        );
        let size_drift =
            (after.semi_major_axis - before.semi_major_axis).abs() / before.semi_major_axis;
        assert!(
            size_drift < 1.0e-9,
            "the semi-major axis drifted by {size_drift} of itself"
        );
        assert!(
            after.eccentricity < 1.0e-9,
            "a circle stayed a circle only to e = {}",
            after.eccentricity
        );

        // And the phase, which is what a symplectic integrator loses while
        // keeping the energy: chaining must land where one direct propagation
        // over the same total time does.
        let direct = propagate(EARTH_MU, start, step * f64::from(STEPS));
        let apart = chained.position.distance(direct.position);
        assert!(
            apart < RADIUS * 1.0e-5,
            "{STEPS} chained propagations ended {apart} m from the single one covering the same time"
        );
    }

    /// An escape trajectory is a different conic and the same code path. It has
    /// no period and no apoapsis, and saying so is the point — a `0.0` there
    /// would read as "it comes back to the centre".
    #[test]
    fn an_escape_trajectory_has_no_period_and_reverses_exactly() {
        const PERIAPSIS: f64 = 6_571_000.0;
        let escape = State::new(
            WorldPos::from_offset(DVec3::new(PERIAPSIS, 0.0, 0.0)),
            // Half again the speed that just barely escapes.
            DVec3::new(0.0, 1.5 * (2.0 * EARTH_MU / PERIAPSIS).sqrt(), 0.0),
        );
        let orbit = Orbit::from_state(EARTH_MU, escape);
        assert!(orbit.eccentricity > 1.0, "e = {}", orbit.eccentricity);
        assert!(!orbit.is_closed());
        assert_eq!(orbit.period(EARTH_MU), None);
        assert_eq!(orbit.apoapsis(), None);
        assert!(
            (orbit.periapsis() - PERIAPSIS).abs() < 1.0e-3,
            "a hyperbola still has a periapsis, got {}",
            orbit.periapsis()
        );

        let day = 86_400.0;
        let out = propagate(EARTH_MU, escape, day);
        assert!(
            radius_of(out) > radius_of(escape) * 10.0,
            "a day of escape should be a long way out, got {} m",
            radius_of(out)
        );
        let back = propagate(EARTH_MU, out, -day);
        let error = back.position.distance(escape.position);
        assert!(
            error < radius_of(out) * 1.0e-9,
            "out and back over {} m of hyperbola came home {error} m adrift",
            radius_of(out)
        );
    }

    /// **Backwards on a near-parabolic orbit stays backwards.**
    ///
    /// Such an orbit closes, but only just: its semi-major axis comes from a
    /// difference of two nearly equal numbers and carries a large relative
    /// error, and its period is astronomical. Folding a small negative time
    /// into the positive remainder would rewrite one second backwards as
    /// almost a whole revolution forwards — exact on paper, and out by
    /// kilometres with a period known to seven figures.
    #[test]
    fn a_backwards_step_on_a_near_parabolic_orbit_does_not_go_the_long_way() {
        const PERIAPSIS: f64 = 6_571_000.0;
        let escape_speed = (2.0 * EARTH_MU / PERIAPSIS).sqrt();
        let barely = State::new(
            WorldPos::from_offset(DVec3::new(PERIAPSIS, 0.0, 0.0)),
            DVec3::new(0.0, escape_speed * (1.0 - 1.0e-9), 0.0),
        );
        let orbit = Orbit::from_state(EARTH_MU, barely);
        assert!(
            orbit.is_closed() && orbit.eccentricity > 0.999,
            "the fixture must be a barely-closed orbit, got e = {}",
            orbit.eccentricity
        );

        // The periapsis has to survive the near-parabolic case too. `a(1 - e)`
        // would be an enormous semi-major axis times a difference of two
        // numbers that agree to nine figures, which keeps almost none of them;
        // the semi-latus rectum is finite and well conditioned here.
        assert!(
            (orbit.periapsis() - PERIAPSIS).abs() < 1.0e-3,
            "a barely-closed orbit still has a periapsis at {PERIAPSIS} m, got {} m",
            orbit.periapsis()
        );

        let hour = 3_600.0;
        let out = propagate(EARTH_MU, barely, hour);
        let back = propagate(EARTH_MU, out, -hour);
        let error = back.position.distance(barely.position);
        assert!(
            error < 1.0e-3,
            "an hour out and back came home {error} m adrift"
        );
    }

    /// A whole number of revolutions is the identity, because that is what the
    /// fold in [`propagate`] makes it — not an accuracy result but the
    /// behaviour timewarp depends on.
    #[test]
    fn a_whole_revolution_returns_the_state_it_started_from() {
        let start = circular(7_000_000.0);
        let period = Orbit::from_state(EARTH_MU, start)
            .period(EARTH_MU)
            .expect("a circular orbit closes");
        let round = propagate(EARTH_MU, start, period);
        assert_eq!(round.position, start.position);
        assert_eq!(round.velocity, start.velocity);
    }

    #[test]
    #[should_panic(expected = "a body at the centre of attraction is not on an orbit")]
    fn a_body_on_the_centre_of_attraction_is_refused() {
        let _ = propagate(EARTH_MU, State::AT_ORIGIN, 1.0);
    }

    #[test]
    #[should_panic(expected = "cannot propagate by NaN seconds")]
    fn a_non_finite_flight_time_is_refused() {
        let _ = propagate(EARTH_MU, circular(7_000_000.0), f64::NAN);
    }
}
