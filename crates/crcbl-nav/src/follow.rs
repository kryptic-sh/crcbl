//! Walking a planned route: which waypoint to steer toward, and whether the
//! walk has stalled.
//!
//! Both came from EW, whose AI followed its routes this way before it moved
//! here; both are pure and deterministic, a function of the feet positions and
//! times the caller hands them, with no clock of their own.
//!
//! * [`RouteFollower`]: the next waypoint of a [`GroundRoute`] to steer
//!   toward, and the stop distance to walk to it with. A waypoint short of the
//!   route's last is dropped only once the feet come within
//!   [`FollowDistances::waypoint_reach_m`] of it, so a walk turns where the
//!   route turns — round a doorway's jamb — rather than making straight for a
//!   waypoint past the turn. Dropping every waypoint within the caller's own
//!   stop distance cut such corners into the jamb, and the walker froze there.
//! * [`StallDetector`]: whether a walk has made no headway — got
//!   [`StallDetector::headway_m`] from where it last did — for
//!   [`StallDetector::timeout_s`], counting only time the caller says it was
//!   walking. A per-tick speed floor never fires for an agent jostling against
//!   another, which moves every tick and goes nowhere.
//!
//! Distances here are over the ground, X and Z, ignoring height: feet partway
//! up a stair stand above or below the floor heights a route's waypoints are
//! at, and a waypoint is reached when the body is beside it, not level with it.
//!
//! What a walker does about a stall — back off, re-plan, give the route up, or
//! wait its turn at a doorway — is a game's rule, and stays the game's.

use glam::DVec3;

use crate::grid::GroundRoute;

/// How near the feet come to a waypoint short of a route's last before a
/// [`RouteFollower`] makes for the next: about a route cell, so a walk follows
/// its route round a corner rather than cutting it. EW's value, proven on its
/// generated maps.
pub const WAYPOINT_REACH_M: f64 = 0.4;

/// The stop distance a [`RouteFollower`] walks to a waypoint short of the
/// route's last with: below [`WAYPOINT_REACH_M`], so a walker that stops at it
/// has reached the waypoint. EW's value.
pub const WAYPOINT_STOP_M: f64 = 0.2;

/// How far a walk must get from where it last made headway to have made some
/// again: far enough that jostling in place never counts. EW's value.
pub const STALL_HEADWAY_M: f64 = 0.5;

/// How long a walk may make no headway, walking, before a [`StallDetector`]
/// calls it stalled, in seconds. EW's value.
pub const STALL_TIMEOUT_S: f64 = 3.0;

/// The distances a [`RouteFollower`] follows a route by.
///
/// The fields are public and nothing is checked: a reach that is negative or
/// NaN never counts a waypoint reached, so the walk never moves past it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FollowDistances {
    /// How near the feet come to a waypoint short of the last before it is
    /// dropped, in metres.
    pub waypoint_reach_m: f64,
    /// The stop distance to walk to a waypoint short of the last with, in
    /// metres; below `waypoint_reach_m`, or a walker that stops at it never
    /// reaches the waypoint.
    pub waypoint_stop_m: f64,
    /// The caller's stop distance for the route's last waypoint, in metres:
    /// both the stop distance it is walked to with and the reach at which the
    /// route is done.
    pub last_stop_m: f64,
}

impl FollowDistances {
    /// [`WAYPOINT_REACH_M`] and [`WAYPOINT_STOP_M`] on the way, and
    /// `last_stop_m` at the end.
    #[must_use]
    pub const fn new(last_stop_m: f64) -> Self {
        Self {
            waypoint_reach_m: WAYPOINT_REACH_M,
            waypoint_stop_m: WAYPOINT_STOP_M,
            last_stop_m,
        }
    }

    /// The reach at which waypoint `index` of a route of `count` counts as
    /// reached: [`last_stop_m`](Self::last_stop_m) for the last,
    /// [`waypoint_reach_m`](Self::waypoint_reach_m) for the rest.
    ///
    /// The reach [`RouteFollower`] drops waypoints by, so a forecast of when a
    /// walk arrives somewhere along the route uses the same one per waypoint.
    #[must_use]
    pub fn reach_for(&self, index: usize, count: usize) -> f64 {
        if index + 1 < count {
            self.waypoint_reach_m
        } else {
            self.last_stop_m
        }
    }
}

/// The waypoint a [`RouteFollower`] steers toward now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steer {
    /// The waypoint to walk to.
    pub waypoint: DVec3,
    /// The stop distance to walk to it with.
    pub stop_distance_m: f64,
    /// How near the feet must come for it to count as reached — what an
    /// arrival forecast measures to.
    pub reach_m: f64,
    /// Whether it is the route's last waypoint.
    pub is_last: bool,
}

/// A planned route being walked: the waypoints not yet reached, and the
/// distances that decide when one is.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteFollower {
    waypoints: Vec<DVec3>,
    /// The index of the first waypoint not yet reached.
    next: usize,
    distances: FollowDistances,
}

impl RouteFollower {
    /// A follower at the start of `route`, which it walks by `distances`.
    #[must_use]
    pub fn new(route: GroundRoute, distances: FollowDistances) -> Self {
        Self {
            waypoints: route.waypoints,
            next: 0,
            distances,
        }
    }

    /// Drops every waypoint reached from `feet`, in order, and answers the
    /// first one that is not, or `None` once the last is reached — the route
    /// is walked.
    ///
    /// A waypoint is reached when it lies within
    /// [`FollowDistances::reach_for`] of `feet` over the ground. Waypoints are
    /// dropped only from the front, so one the feet passed beside without
    /// reaching still holds the walk until it is reached; and once dropped one
    /// never comes back, whatever `feet` does next.
    pub fn advance(&mut self, feet: DVec3) -> Option<Steer> {
        let count = self.waypoints.len();
        while self.next < count
            && ground_distance(feet, self.waypoints[self.next])
                <= self.distances.reach_for(self.next, count)
        {
            self.next += 1;
        }
        let waypoint = *self.waypoints.get(self.next)?;
        let is_last = self.next + 1 == count;
        Some(Steer {
            waypoint,
            stop_distance_m: if is_last {
                self.distances.last_stop_m
            } else {
                self.distances.waypoint_stop_m
            },
            reach_m: self.distances.reach_for(self.next, count),
            is_last,
        })
    }

    /// The waypoints not yet reached, in order.
    #[must_use]
    pub fn remaining(&self) -> &[DVec3] {
        &self.waypoints[self.next..]
    }

    /// Whether every waypoint has been reached: an empty route is walked from
    /// the start.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.next == self.waypoints.len()
    }

    /// The distances the route is followed by.
    #[must_use]
    pub fn distances(&self) -> FollowDistances {
        self.distances
    }
}

/// Whether a walk is making headway, measured from an anchor: where the feet
/// stood when it last made some.
///
/// Each [`tick`](Self::tick) re-anchors at the feet when they are
/// [`headway_m`](Self::headway_m) or farther from the anchor, and also
/// whenever the caller says it is not walking — waiting at a doorway for
/// another to pass, or holding cover — so waiting never counts toward a stall
/// and the clock starts afresh from where the walk resumes. Otherwise the
/// tick's time adds to the time since the last headway, and the walk is
/// stalled once that reaches [`timeout_s`](Self::timeout_s). It stays stalled,
/// tick after tick, until it makes headway, stops walking or is
/// [`reset`](Self::reset).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StallDetector {
    /// How far from the anchor the feet must get to have made headway, in
    /// metres.
    pub headway_m: f64,
    /// How long a walk may make no headway before it is stalled, in seconds.
    pub timeout_s: f64,
    anchor: Option<DVec3>,
    since_headway_s: f64,
}

impl Default for StallDetector {
    /// [`STALL_HEADWAY_M`] and [`STALL_TIMEOUT_S`].
    fn default() -> Self {
        Self::new(STALL_HEADWAY_M, STALL_TIMEOUT_S)
    }
}

impl StallDetector {
    /// A detector with no anchor yet, so its first tick anchors.
    #[must_use]
    pub const fn new(headway_m: f64, timeout_s: f64) -> Self {
        Self {
            headway_m,
            timeout_s,
            anchor: None,
            since_headway_s: 0.0,
        }
    }

    /// Measures one tick of `dt_s` seconds that ended with the feet at
    /// `feet`, while the caller was `walking` its route or not, and answers
    /// whether the walk is stalled.
    ///
    /// A `dt_s` that is zero, negative or not finite is no time passed — a
    /// preview of the walk, not a step of it — and changes nothing.
    pub fn tick(&mut self, feet: DVec3, walking: bool, dt_s: f64) -> bool {
        if !(dt_s.is_finite() && dt_s > 0.0) {
            return self.is_stalled();
        }
        let headway = self
            .anchor
            .is_none_or(|anchor| ground_distance(anchor, feet) >= self.headway_m);
        if !walking || headway {
            self.anchor = Some(feet);
            self.since_headway_s = 0.0;
        } else {
            self.since_headway_s += dt_s;
        }
        self.is_stalled()
    }

    /// Whether the walk has made no headway, walking, for the timeout.
    #[must_use]
    pub fn is_stalled(&self) -> bool {
        self.since_headway_s >= self.timeout_s
    }

    /// Where the feet stood when the walk last made headway, or stopped
    /// walking; `None` before the first tick.
    #[must_use]
    pub fn anchor(&self) -> Option<DVec3> {
        self.anchor
    }

    /// How long the walk has made no headway, walking, in seconds.
    #[must_use]
    pub fn since_headway_s(&self) -> f64 {
        self.since_headway_s
    }

    /// Forgets the anchor and the time, as for a new route: the next tick
    /// anchors afresh.
    pub fn reset(&mut self) {
        self.anchor = None;
        self.since_headway_s = 0.0;
    }
}

/// The distance from `a` to `b` over the ground, ignoring height.
fn ground_distance(a: DVec3, b: DVec3) -> f64 {
    (b.x - a.x).hypot(b.z - a.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{GridCell, GroundGrid, GroundGridConfig};

    const CELL_M: f64 = 0.5;

    /// A 5 × 5 field of half-metre cells with a wall along row 2 and a doorway
    /// through it at column 2.
    fn walled_field() -> GroundGrid {
        let mut cells = vec![GridCell::Walkable; 25];
        for column in [0, 1, 3, 4] {
            cells[2 * 5 + column] = GridCell::Blocked;
        }
        GroundGrid::new(GroundGridConfig::new(DVec3::ZERO, CELL_M, 5, 5), cells).unwrap()
    }

    fn centre(column: usize, row: usize) -> DVec3 {
        DVec3::new(
            (column as f64 + 0.5) * CELL_M,
            0.0,
            (row as f64 + 0.5) * CELL_M,
        )
    }

    /// Whether the straight walk from `a` to `b` crosses a blocked cell.
    fn crosses_blocked(grid: &GroundGrid, a: DVec3, b: DVec3) -> bool {
        (0..=200).any(|step| {
            let point = a.lerp(b, f64::from(step) / 200.0);
            grid.plan(point, point).is_err()
        })
    }

    /// **The corner waypoint holds the walk until the reach, not the stop
    /// distance.** EW's motivating case: an L-shaped route along a wall and
    /// through its doorway, walked with a 1 m stop distance. Dropping every
    /// waypoint within that stop distance makes straight for a waypoint past
    /// the doorway, across the jamb; the follower makes for the corner, and
    /// drops it only within [`WAYPOINT_REACH_M`].
    #[test]
    fn an_l_shaped_route_through_a_doorway_turns_at_its_corner() {
        let grid = walled_field();
        let route = grid.plan(centre(0, 1), centre(2, 4)).unwrap();
        let corner = centre(2, 1);
        assert_eq!(
            route.waypoints,
            vec![
                centre(1, 1),
                corner,
                centre(2, 2),
                centre(2, 3),
                centre(2, 4)
            ],
            "along the wall, then through the doorway"
        );
        let stop = 1.0;
        let mut follower = RouteFollower::new(route.clone(), FollowDistances::new(stop));

        // In cell (1, 1), short of the wall: the corner is within the stop
        // distance and past the reach.
        let feet = DVec3::new(0.75, 0.0, 0.9);
        let to_corner = ground_distance(feet, corner);
        assert!(to_corner < stop && to_corner > WAYPOINT_REACH_M);
        let naive = route
            .waypoints
            .iter()
            .copied()
            .find(|&point| ground_distance(feet, point) > stop)
            .unwrap();
        assert!(
            crosses_blocked(&grid, feet, naive),
            "dropping by the stop distance makes for {naive} across the jamb"
        );

        let steer = follower.advance(feet).unwrap();
        assert_eq!(
            steer,
            Steer {
                waypoint: corner,
                stop_distance_m: WAYPOINT_STOP_M,
                reach_m: WAYPOINT_REACH_M,
                is_last: false,
            }
        );
        assert!(!crosses_blocked(&grid, feet, steer.waypoint));
        assert_eq!(follower.remaining(), &route.waypoints[1..]);

        // Just outside the reach the corner still holds; just inside, the
        // doorway is next.
        let outside = corner - DVec3::X * (WAYPOINT_REACH_M + 0.01);
        assert_eq!(follower.advance(outside).unwrap().waypoint, corner);
        let inside = corner - DVec3::X * (WAYPOINT_REACH_M - 0.01);
        let steer = follower.advance(inside).unwrap();
        assert_eq!(steer.waypoint, centre(2, 2));
        assert!(!crosses_blocked(&grid, inside, steer.waypoint));
    }

    /// **The last waypoint is walked to with the caller's stop distance, and
    /// reached at it.**
    #[test]
    fn the_last_waypoint_uses_the_callers_stop_distance() {
        let stop = 1.0;
        let first = DVec3::new(1.0, 0.0, 0.0);
        let last = DVec3::new(1.0, 0.0, 3.0);
        let mut follower = RouteFollower::new(
            GroundRoute {
                waypoints: vec![first, last],
            },
            FollowDistances::new(stop),
        );
        assert!(!follower.is_finished());

        let steer = follower.advance(first).unwrap();
        assert_eq!(
            steer,
            Steer {
                waypoint: last,
                stop_distance_m: stop,
                reach_m: stop,
                is_last: true,
            }
        );
        // Past the waypoint reach, inside the stop distance: arrived.
        let short = last - DVec3::Z * (stop * 0.9);
        assert!(ground_distance(short, last) > WAYPOINT_REACH_M);
        assert_eq!(follower.advance(short), None);
        assert!(follower.is_finished());
        assert!(follower.remaining().is_empty());
        assert_eq!(
            follower.advance(DVec3::ZERO),
            None,
            "a dropped waypoint never comes back"
        );
    }

    /// **Distances are over the ground**: feet a stair's height off a
    /// waypoint, beside it, have reached it.
    #[test]
    fn a_waypoint_is_reached_beside_it_whatever_the_height() {
        let below = DVec3::new(0.0, 0.0, 0.0);
        let next = DVec3::new(2.0, 0.0, 0.0);
        let mut follower = RouteFollower::new(
            GroundRoute {
                waypoints: vec![below + DVec3::Y * 2.0, next],
            },
            FollowDistances::new(0.5),
        );
        assert_eq!(follower.advance(below).unwrap().waypoint, next);
    }

    #[test]
    fn an_empty_route_is_walked_from_the_start() {
        let mut follower = RouteFollower::new(
            GroundRoute {
                waypoints: Vec::new(),
            },
            FollowDistances::new(1.0),
        );
        assert!(follower.is_finished());
        assert_eq!(follower.advance(DVec3::ZERO), None);
    }

    /// The reach a forecast reads is the one the follower drops by: the
    /// waypoint reach on the way, the stop distance at the end.
    #[test]
    fn the_reach_per_waypoint_is_the_waypoint_reach_until_the_last() {
        let distances = FollowDistances::new(1.5);
        assert_eq!(distances.reach_for(0, 3), WAYPOINT_REACH_M);
        assert_eq!(distances.reach_for(1, 3), WAYPOINT_REACH_M);
        assert_eq!(distances.reach_for(2, 3), 1.5);
        assert_eq!(distances.reach_for(0, 1), 1.5);
        assert_eq!(
            distances,
            FollowDistances {
                waypoint_reach_m: 0.4,
                waypoint_stop_m: 0.2,
                last_stop_m: 1.5,
            },
            "EW's values"
        );
    }

    const TICK_S: f64 = 0.5;

    /// Ticks an anchored `detector` with `feet` and `walking` until just
    /// short of the default timeout, asserting no stall, then once more,
    /// answering it.
    fn tick_through_timeout(
        detector: &mut StallDetector,
        mut feet: impl FnMut(usize) -> DVec3,
        walking: bool,
    ) -> bool {
        let ticks = (STALL_TIMEOUT_S / TICK_S) as usize - 1;
        for tick in 0..ticks {
            assert!(
                !detector.tick(feet(tick), walking, TICK_S),
                "stalled at tick {tick}"
            );
        }
        detector.tick(feet(ticks), walking, TICK_S)
    }

    /// **Jostling in place while walking stalls after the timeout**: the feet
    /// move every tick, a tenth of the headway back and forth, and never get
    /// the headway from the anchor.
    #[test]
    fn jostling_in_place_while_walking_stalls_after_the_timeout() {
        let mut detector = StallDetector::default();
        assert!(
            !detector.tick(DVec3::ZERO, true, TICK_S),
            "the first anchors"
        );
        let jostle = |tick: usize| DVec3::X * (STALL_HEADWAY_M * 0.1 * (tick % 2) as f64);
        assert!(tick_through_timeout(&mut detector, jostle, true));
        assert_eq!(detector.since_headway_s(), STALL_TIMEOUT_S);
        assert_eq!(detector.anchor(), Some(DVec3::ZERO));
        assert!(detector.tick(DVec3::ZERO, true, TICK_S), "it stays stalled");

        detector.reset();
        assert_eq!(detector.anchor(), None);
        assert!(!detector.tick(DVec3::ZERO, true, TICK_S));
    }

    /// **Waiting never stalls**, however long, and walking on afterward counts
    /// from where it resumed.
    #[test]
    fn waiting_never_stalls() {
        let mut detector = StallDetector::default();
        for _ in 0..100 {
            assert!(!detector.tick(DVec3::ZERO, false, TICK_S));
        }
        assert_eq!(detector.since_headway_s(), 0.0);
        let resumed = DVec3::new(0.1, 0.0, 0.0);
        assert!(!detector.tick(resumed, false, TICK_S));
        assert!(tick_through_timeout(&mut detector, |_| resumed, true));
    }

    /// **Steady progress never stalls**, and each headway re-anchors.
    #[test]
    fn steady_progress_never_stalls() {
        let mut detector = StallDetector::default();
        // A slow walk: a fifth of the headway a tick.
        let step = STALL_HEADWAY_M * 0.2;
        for tick in 0..100 {
            let feet = DVec3::Z * (step * f64::from(tick));
            assert!(!detector.tick(feet, true, TICK_S), "stalled at {feet}");
        }
        let anchor = detector.anchor().unwrap();
        assert!(anchor.z > 99.0 * step - STALL_HEADWAY_M, "{anchor}");
    }

    /// **Headway re-anchors and restarts the clock**: a walk that gets the
    /// headway just before the timeout stalls a whole timeout later, measured
    /// from the new anchor.
    #[test]
    fn headway_re_anchors_and_restarts_the_clock() {
        let mut detector = StallDetector::default();
        let ticks = (STALL_TIMEOUT_S / TICK_S) as usize;
        for _ in 0..ticks {
            assert!(!detector.tick(DVec3::ZERO, true, TICK_S));
        }
        let ahead = DVec3::new(0.0, 5.0, STALL_HEADWAY_M);
        assert!(!detector.tick(ahead, true, TICK_S));
        assert_eq!(detector.anchor(), Some(ahead));
        assert_eq!(detector.since_headway_s(), 0.0);

        // Short of the headway from the new anchor, though past it from the
        // old one, and climbing doesn't count.
        let near = ahead + DVec3::new(0.0, -5.0, STALL_HEADWAY_M * 0.9);
        assert!(tick_through_timeout(&mut detector, |_| near, true));
        assert_eq!(detector.anchor(), Some(ahead));
    }

    /// A tick with no time passed — a preview — changes nothing.
    #[test]
    fn a_tick_with_no_time_changes_nothing() {
        let mut detector = StallDetector::default();
        assert!(!detector.tick(DVec3::ZERO, true, TICK_S));
        let before = detector;
        for dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(!detector.tick(DVec3::X * 9.0, false, dt), "{dt}");
            assert_eq!(detector, before, "{dt}");
        }
        assert!(tick_through_timeout(&mut detector, |_| DVec3::ZERO, true));
    }
}
