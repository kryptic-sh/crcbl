//! `PathSystem`: where a creep is, `s` metres into the map's [`Path`].
//!
//! ```text
//!   along ──▶ point_at ──▶ DVec3   (where the sphere is written)
//!         └─▶ heading_at ─▶ yaw    (which way it is walking)
//! ```
//!
//! # This is a polyline follower and not a spline follower
//!
//! `docs/plan/sample/07-towers.md` asks for creeps that "walk spline", and the
//! engine has no spline type to walk: `crcbl-phys` and `crcbl-scene` offer
//! none, and the only splines in the workspace are `crcbl-anim`'s clip
//! interpolation and the glTF importer's, neither of which is a curve a game
//! can put a body on. So this module measures **straight legs between
//! waypoints**, which is sample code over kinematic bodies exactly as that
//! document predicted. The gap is recorded in `docs/backlog.md` rather than
//! papered over here; what a spline would change is the corners, where a creep
//! currently turns in one tick.
//!
//! # Distance is the state, not a leg index and a fraction
//!
//! A creep carries one number, the metres it has walked, and every reading is a
//! function of it. That is what makes a creep's position a pure function of its
//! own clock — two runs that spawned the same creep at the same tick put it in
//! the same place — and it is why [`Path::point_at`] asks the path and nothing
//! about the creep but that one number.

use crcbl::math::DVec3;

use crate::map::{HALF_DEPTH, HALF_WIDTH, LANE_WIDTH, MAX_WAYPOINTS, MapError};

/// The waypoints a creep walks, spawn first and exit last, checked.
///
/// **Only [`Path::new`] builds one**, and it refuses every list this module's
/// arithmetic cannot walk: fewer than two waypoints is a path with no leg to
/// stand on, and a diagonal or climbing leg is one the lane meshes in
/// `crate::map` cannot draw as one `platform`. So every method below can index
/// a leg and assume it is flat and straight without a guard of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    waypoints: Vec<DVec3>,
}

impl Path {
    /// The path through `waypoints`, in the order given.
    ///
    /// # Errors
    ///
    /// [`MapError`], naming the waypoint or the leg it is about: fewer than two
    /// waypoints or more than [`MAX_WAYPOINTS`], a waypoint off the ground or
    /// off the field, and a leg that is diagonal or shorter than the lane is
    /// wide.
    pub fn new(waypoints: Vec<DVec3>) -> Result<Self, MapError> {
        if waypoints.len() < 2 {
            return Err(MapError::TooFewWaypoints {
                found: waypoints.len(),
            });
        }
        if waypoints.len() > MAX_WAYPOINTS {
            return Err(MapError::TooManyWaypoints {
                found: waypoints.len(),
            });
        }
        for (index, point) in waypoints.iter().enumerate() {
            let what = || format!("waypoint {index}");
            // The ground's top is `y = 0` and the lane is drawn there, so a
            // waypoint above or below it is a creep walking on air or in the
            // slab beside a lane that says otherwise.
            if point.y != 0.0 {
                return Err(MapError::OffTheGround {
                    what: what(),
                    y: point.y,
                });
            }
            if point.x.abs() + 0.5 * LANE_WIDTH > HALF_WIDTH
                || point.z.abs() + 0.5 * LANE_WIDTH > HALF_DEPTH
            {
                return Err(MapError::OffTheField { what: what() });
            }
        }
        for (leg, pair) in waypoints.windows(2).enumerate() {
            let step = pair[1] - pair[0];
            // Measured before the axis test, so a leg of zero length is named
            // for what it is rather than as a diagonal.
            if step.length() <= LANE_WIDTH {
                return Err(MapError::ShortLeg {
                    leg,
                    length: step.length(),
                });
            }
            if (step.x == 0.0) == (step.z == 0.0) {
                return Err(MapError::Diagonal { leg });
            }
        }
        Ok(Self { waypoints })
    }

    /// The waypoints, spawn first.
    #[must_use]
    pub fn waypoints(&self) -> &[DVec3] {
        &self.waypoints
    }

    /// How many straight legs the path has: one fewer than its waypoints.
    #[must_use]
    pub fn legs(&self) -> usize {
        self.waypoints.len() - 1
    }

    /// The last waypoint, which is where the exit stands.
    #[must_use]
    pub fn end(&self) -> DVec3 {
        self.waypoints[self.waypoints.len() - 1]
    }

    /// How long one leg is, in metres.
    ///
    /// # Panics
    ///
    /// If `leg` is not a leg. Every caller here iterates `0..legs()`.
    #[must_use]
    pub fn leg_length(&self, leg: usize) -> f64 {
        (self.waypoints[leg + 1] - self.waypoints[leg]).length()
    }

    /// How long the whole path is, in metres.
    #[must_use]
    pub fn length(&self) -> f64 {
        (0..self.legs()).map(|leg| self.leg_length(leg)).sum()
    }

    /// Which leg `s` metres in falls on, and how far along that leg it is.
    ///
    /// Clamped at both ends: before the start is the first leg at zero, past the
    /// finish is the last leg at its full length. A creep is taken off the field
    /// by the exit volume rather than by running out of path — see
    /// [`crate::creep::has_reached_the_exit`] — so the far clamp is a guard
    /// rather than a state a run reaches.
    fn leg_of(&self, s: f64) -> (usize, f64) {
        let legs = self.legs();
        let mut left = s.max(0.0);
        for leg in 0..legs {
            let span = self.leg_length(leg);
            if left <= span || leg + 1 == legs {
                return (leg, left.min(span));
            }
            left -= span;
        }
        // `Path::new` refuses fewer than two waypoints, so there is at least one
        // leg and the loop above always returns.
        unreachable!("the path has no legs")
    }

    /// Where a creep `s` metres along the path stands, on the ground.
    #[must_use]
    pub fn point_at(&self, s: f64) -> DVec3 {
        let (leg, along) = self.leg_of(s);
        let step = self.waypoints[leg + 1] - self.waypoints[leg];
        self.waypoints[leg] + step.normalize_or_zero() * along
    }

    /// Which way a creep `s` metres along the path is walking, as a yaw in
    /// `apps/breach::camera::forward`'s measure — zero looks down `-Z` and a
    /// rising yaw swings toward `+X`.
    ///
    /// Read by the frame and by nothing in the simulation: a creep on a polyline
    /// has no steering to do.
    #[must_use]
    pub fn heading_at(&self, s: f64) -> f64 {
        let (leg, _) = self.leg_of(s);
        let step = self.waypoints[leg + 1] - self.waypoints[leg];
        step.x.atan2(-step.z)
    }

    /// How far `point` is from the nearest point on the path's centre line, in
    /// metres — what `crate::map` holds a build plot's clearance and reach to.
    #[must_use]
    pub fn distance_to(&self, point: DVec3) -> f64 {
        self.waypoints
            .windows(2)
            .map(|pair| {
                let (from, step) = (pair[0], pair[1] - pair[0]);
                let t = ((point - from).dot(step) / step.length_squared()).clamp(0.0, 1.0);
                (from + step * t - point).length()
            })
            .fold(f64::INFINITY, f64::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed field's path, which is what every reading here is taken
    /// along.
    fn path() -> Path {
        crate::map::Map::built_in().path().clone()
    }

    /// One centimetre, which is the tolerance every comparison here is made to.
    const EPS: f64 = 1e-2;

    /// **The two ends of the path are the two ends of the waypoint list**, and
    /// the length is the legs added up. The claim every other reading rests on.
    #[test]
    fn the_ends_of_the_walk_are_the_ends_of_the_list() {
        let path = path();
        let waypoints = path.waypoints();
        assert!((path.point_at(0.0) - waypoints[0]).length() < EPS);
        assert!((path.point_at(path.length()) - waypoints[waypoints.len() - 1]).length() < EPS);
        // Clamped rather than extrapolated at both ends.
        assert!((path.point_at(-5.0) - waypoints[0]).length() < EPS);
        assert!(
            (path.point_at(path.length() + 50.0) - waypoints[waypoints.len() - 1]).length() < EPS
        );

        let legs: f64 = (0..path.legs()).map(|leg| path.leg_length(leg)).sum();
        assert!((path.length() - legs).abs() < EPS);
    }

    /// **Every waypoint is somewhere the walk actually passes through**, at the
    /// distance the legs before it add up to. A follower that skipped a corner
    /// — the failure a leg index off by one produces — puts a creep on the
    /// diagonal between two legs and passes an end-to-end test unchanged.
    #[test]
    fn the_walk_passes_through_every_waypoint() {
        let path = path();
        let mut so_far = 0.0;
        for (leg, waypoint) in path.waypoints().iter().enumerate() {
            assert!(
                (path.point_at(so_far) - *waypoint).length() < EPS,
                "waypoint {leg} is not {so_far:.2} m in",
            );
            if leg < path.legs() {
                so_far += path.leg_length(leg);
            }
        }
        assert!(
            (so_far - path.length()).abs() < EPS,
            "the waypoints do not add up to the path",
        );
    }

    /// **Walking is monotone and at the speed asked for**: a step of `d` metres
    /// moves a creep `d` metres, except across a corner, where the polyline
    /// turns and the straight-line distance is shorter.
    ///
    /// Asserted over the whole path in ten-centimetre steps, so the corners are
    /// in the sample rather than avoided by it.
    #[test]
    fn a_step_along_the_path_covers_the_distance_it_asks_for() {
        const STEP: f64 = 0.1;
        let path = path();
        let mut travelled = 0.0;
        let mut s = 0.0;
        while s + STEP <= path.length() {
            let moved = (path.point_at(s + STEP) - path.point_at(s)).length();
            assert!(
                moved <= STEP + EPS,
                "a {STEP} m step covered {moved:.4} m at s = {s:.2}",
            );
            travelled += moved;
            s += STEP;
        }
        // The corners are the only place the two disagree, and there is one
        // fewer of them than there are legs, each losing under one step.
        assert!(
            travelled > path.length() - (path.legs() as f64) * STEP - EPS,
            "walking the path in {STEP} m steps covered {travelled:.2} m of {:.2}",
            path.length(),
        );
    }

    /// **The heading is the leg's own direction**, which is what the frame
    /// turns a creep by. One reading per leg, taken at its middle so a corner
    /// cannot answer for its neighbour.
    #[test]
    fn the_heading_is_the_leg_the_creep_is_on() {
        let path = path();
        let mut so_far = 0.0;
        for (leg, pair) in path.waypoints().windows(2).enumerate() {
            let span = path.leg_length(leg);
            let step = pair[1] - pair[0];
            let expected = step.x.atan2(-step.z);
            let read = path.heading_at(so_far + 0.5 * span);
            assert!(
                (read - expected).abs() < 1e-9,
                "leg {leg} reads {read} and runs at {expected}",
            );
            so_far += span;
        }
    }
}
