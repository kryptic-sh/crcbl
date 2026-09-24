//! Continuous collision: contact-solver rung 4 (`docs/notes/simulation.md`),
//! decision 5 — speculative contacts for everything, then sweeps for fast
//! bodies, with the time they lose dropped.
//!
//! ```text
//!   solve ─▶ for each awake dynamic body, non-bullets first:
//!              moved ≥ ½ its inner radius this tick, or a bullet that moved?
//!                │ yes
//!              the path's bounds ─▶ static proxies and planes
//!                                   (a bullet: every other body too)
//!              each part × each shape: time of impact, by conservative
//!              advancement along start → end
//!                │ the earliest, if any
//!              put the body there; the rest of its tick is dropped
//! ```
//!
//! # Why a sweep after the solve
//!
//! Speculative contacts, rung 1's, stop a body that is heading for a surface
//! the manifold saw when the tick began. What they cannot stop is a surface
//! the manifold did not see: a cube spinning at 80 rad/s turns a corner it
//! did not have near the peg when the tick began, so no point of the
//! manifold is on it, and the corner swings through. The sweep looks at where
//! the body went instead of where it was, so it sees that corner. It runs
//! once, after the solve, and only for bodies that moved far for their size:
//! the single pass Box2D v3 (`solver.c`, `b2FinalizeBodiesTask` choosing the
//! fast bodies and `b2SolveContinuous` sweeping each), Jolt and PhysX all
//! make.
//!
//! # Which bodies, against what
//!
//! A body is **fast** when its path over the tick — its centre's travel plus
//! the most its parts' cores turned — reaches half its inner radius: the
//! radius of a sphere or a capsule, a box's smallest half-extent, the least
//! of those over a compound's parts. That is Box2D's test, its
//! `maxVelocity < 0.5 · minExtent` with its `minExtent` the same inner
//! radius, and decision 5's "moved more than half its smallest extent" —
//! except that Box2D turns its shapes' farthest points, where this turns
//! their cores, so a rolling ball is not fast for its spin. A fast
//! body sweeps against static bodies and planes only: two dynamic bodies are
//! each other's speculative contacts' to keep apart.
//!
//! A **bullet** ([`crate::RigidBody::bullet`]) sweeps every tick it moves at
//! all, and against every other body too — dynamic, kinematic or asleep,
//! where each ends the tick — except another bullet. Bullets go after every
//! other fast body, so the bodies they sweep against are already where their
//! own sweeps left them, which is Box2D's order. A sleeping body is not in
//! the awake set and is never swept; a kinematic body goes where it is told
//! and is never swept either.
//!
//! # Time of impact: conservative advancement
//!
//! The path is the body's centre moved in a straight line from where it
//! started the tick to where the solve left it, and its orientation turned by
//! the normalised linear interpolation of the two quaternions — Box2D's
//! `b2GetSweepTransform` and Box3D's, in three dimensions. Along it the time
//! of impact is found by **conservative advancement**: Mirtich, _Impulse-based
//! Dynamic Simulation of Rigid Body Systems_, PhD thesis, UC Berkeley, 1996,
//! as Catto presents it in "Continuous Collision", GDC 2013, and as Bullet's
//! `btContinuousConvexCollision` steps it. At each step:
//!
//! - measure the gap `d` and its normal `n` — [`gap`], exact for round
//!   shapes and a lower bound for two boxes;
//! - bound how fast the gap can close: the travel's part along `-n`, plus the
//!   fastest the part's core can turn, the interpolation's greatest
//!   turning rate times the core's reach from the body's centre (a round
//!   part's surface moves only as far as its core does);
//! - advance by `(d - target) / closing`;
//!
//! until the gap is within [`TOLERANCE`] of the target, or the step passes
//! the earliest impact already found. For the travel that step is safe: the
//! gap between two convex shapes, one moving in a straight line, is convex in
//! time, so a step along its tangent falls short of where it reaches the
//! target. The turning is bounded outright. That pairing, the travel along
//! the normal and the turning in full, is Bullet's, and like Bullet's it is
//! not a proof once both move: the normal the travel is projected on turns
//! too. The turning rate of a normalised
//! interpolation is not constant: between orientations `2α` apart it peaks
//! at `4 tan(α/2)` per tick, midway, against `2α` for a steady turn, and
//! that peak is the bound taken — computed as `4 sin α / (1 + cos α)`, from the
//! quaternion between the two, with no trigonometry.
//!
//! **Two targets: one for whether, one for where.** A path meets a shape if
//! it gets a linear slop into it — or, for a body that began the tick deeper
//! than that, any deeper than it began. A body that meets one having begun
//! the tick clear of it is then stopped where it was a linear slop short, a
//! second advancement to Box2D's own target; one that began nearer is stopped
//! where it got too deep. Box2D has one target, a slop short, and sweeps a
//! small circle at the centroid for a body that begins touching. Both of its
//! choices failed here, measured on the obstacle wall on 2026-09-23:
//!
//! - **Deciding by "a slop short"** stopped most of the bodies the solve had
//!   just landed on a peg or a bin, a few millimetres before they got there —
//!   2046 of them in twenty seconds, dropping 10.9 s of their motion. The
//!   solve had already stopped them; the sweep undid it.
//! - **Stopping "a slop in"** instead left the next tick's contact starting in
//!   overlap, where it is soft, rather than a gap, where a speculative point
//!   is rigid: a shot at 80 m/s into a dead brick came back at 8.9 m/s. Short
//!   of the surface, it stops.
//! - **A circle at the centroid** cannot see a corner turn into the peg it
//!   already touches, which is the case the wall needed; and "no deeper than
//!   it began", rather than a slop deeper each tick, is what stops that corner
//!   ratcheting in a slop a tick while the solve fails to push it out.
//!
//! A body sliding or rolling along a surface gets no deeper, so it is not held
//! on it — Box2D's "pausing".
//!
//! # Dropped time
//!
//! A body stopped at its time of impact keeps its velocity, and the rest of
//! its tick is **dropped**: it is not moved again or solved again this tick,
//! and next tick's speculative contact, built where it stopped, takes it from
//! there. That is decision 5's "lost time is dropped rather than re-solved",
//! and Box2D's, Jolt's and PhysX's practice. [`super::ContactCounters`] sums
//! the dropped share of each stopped body's tick.
//!
//! # What it does not do
//!
//! A dynamic body that is not a bullet is not swept against other dynamic
//! bodies, and no sweep re-solves: two fast bodies meeting mid-air are their
//! speculative contacts' to catch. The interpolated path is not the path the
//! substeps took: a body that bounced within the tick is swept along the
//! straight line between where it began and where it ended, which may cut a
//! corner it went round.

use crcbl_core::Pool;
use glam::{DQuat, DVec3};

use super::broadphase::ProxyId;
use super::island::Islands;
use super::manifold::{LINEAR_SLOP, gap};
use super::shape::ContactShape;
use super::{Bodies, ContactPipeline, Owner, owner_side};
use crate::collider::Aabb;
use crate::components::{ColliderComponent, Transform};
use crate::system::{AwakeSet, BodyRecord, StaticSet};

/// How far a body must go over a tick, as a share of its inner radius, to be
/// swept: Box2D's safety factor.
const FAST_FRACTION: f64 = 0.5;

/// How near its target a gap must come to count as an impact, in metres: a
/// quarter of a linear slop, Box2D's `b2TimeOfImpact` tolerance.
const TOLERANCE: f64 = 0.25 * LINEAR_SLOP;

/// The most steps conservative advancement takes for one pair. A pair still
/// short of the target after this many is stopped where the last step left
/// it, which is short of any impact: Box2D's `b2TimeOfImpact` does the same
/// when it runs out of iterations.
const MAX_ADVANCES: usize = 32;

/// The buffers a sweep works in, kept between ticks.
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    /// Each awake body's transform as the tick began, by awake index.
    starts: Vec<Transform>,
    /// The proxies a path's bounds reach.
    candidates: Vec<ProxyId>,
}

/// A body's path over the tick, as the sweep interpolates it.
#[derive(Clone, Copy, Debug)]
struct Path {
    start: Transform,
    /// The end orientation, on the start's side of the quaternion sphere.
    end_rotation: DQuat,
    /// How far the centre goes.
    travel: DVec3,
    /// The greatest rate the interpolated orientation turns at, in radians
    /// per tick.
    turn_rate: f64,
}

impl Path {
    fn new(start: &Transform, end: &Transform) -> Self {
        let end_rotation = if start.rotation.dot(end.rotation) < 0.0 {
            -end.rotation
        } else {
            end.rotation
        };
        // The turn from start to end, `2α` about some axis: its `w` is
        // `cos α`, not negative on this side of the sphere, and its vector
        // part `sin α` long.
        let turn = end_rotation * start.rotation.conjugate();
        let (cos, sin) = (turn.w.max(0.0), turn.xyz().length());
        Self {
            start: *start,
            end_rotation,
            travel: end.position - start.position,
            turn_rate: 4.0 * sin / (1.0 + cos),
        }
    }

    /// Where the body is a share `t` of the way along.
    fn at(&self, t: f64) -> Transform {
        let from = self.start.rotation;
        Transform::new(
            self.start.position + self.travel * t,
            (from + (self.end_rotation - from) * t).normalize(),
        )
    }
}

/// A collider's size, as the sweep's test for a fast body reads it.
#[derive(Clone, Copy, Debug)]
struct Extent {
    /// The least inner radius over its parts.
    inner: f64,
    /// The furthest any of its points lies from the body's centre.
    reach: f64,
    /// The furthest any point of its parts' cores lies from the body's
    /// centre: see [`core_reach`].
    core_reach: f64,
}

impl Extent {
    /// `component` on a body at `transform`, or `None` for a trigger.
    fn of(component: &ColliderComponent, transform: &Transform) -> Option<Self> {
        let mut extent: Option<Self> = None;
        for part in 0..component.part_count() {
            let Some(shape) = ContactShape::placed_part(component, part, transform) else {
                continue;
            };
            let inner = match shape {
                ContactShape::Sphere { radius, .. } | ContactShape::Capsule { radius, .. } => {
                    radius
                }
                ContactShape::Box { half, .. } => half.min_element(),
                // A mesh is never on a dynamic body, and a plane is no body.
                ContactShape::Triangle { .. } | ContactShape::Plane { .. } => continue,
            };
            let this = Self {
                inner,
                reach: shape.reach_from(transform.position),
                core_reach: core_reach(&shape, transform.position),
            };
            extent = Some(extent.map_or(this, |e| Self {
                inner: e.inner.min(this.inner),
                reach: e.reach.max(this.reach),
                core_reach: e.core_reach.max(this.core_reach),
            }));
        }
        extent
    }
}

/// How far the furthest point of `shape`'s core — a sphere's centre, a
/// capsule's segment, a box's corners — lies from `centre`: what a turn of one
/// radian about `centre` moves the shape by, at most.
///
/// A round shape is its core fattened by its radius, and turning it about a
/// point moves it only as far as it moves its core: a ball spinning about its
/// own centre does not move at all. So a rolling ball's spin, which a bound
/// on its surface's points would count in full, costs its sweep nothing.
fn core_reach(shape: &ContactShape, centre: DVec3) -> f64 {
    match *shape {
        ContactShape::Sphere { centre: c, .. } => (c - centre).length(),
        ContactShape::Capsule { a, b, .. } => (a - centre).length().max((b - centre).length()),
        ContactShape::Box {
            centre: c, half, ..
        } => (c - centre).length() + half.length(),
        ContactShape::Triangle { .. } => shape.reach_from(centre),
        ContactShape::Plane { .. } => 0.0,
    }
}

impl ContactPipeline {
    /// Remembers where every awake body begins the tick, for the sweep after
    /// the solve. The awake set must not change between the two.
    pub(crate) fn remember_starts(&mut self, awake: &AwakeSet) {
        self.continuous.starts.clear();
        self.continuous.starts.extend_from_slice(&awake.transforms);
    }

    /// Sweeps every fast body and every moving bullet over the tick of `dt`
    /// just solved, stopping each at its earliest impact: see the module
    /// docs.
    pub(crate) fn sweep(
        &mut self,
        records: &Pool<BodyRecord>,
        statics: &StaticSet,
        awake: &mut AwakeSet,
        islands: &Islands,
        dt: f64,
    ) {
        self.counters.swept = 0;
        self.counters.sweep_candidates = 0;
        self.counters.sweep_hits = 0;
        self.counters.dropped_time = 0.0;
        if !self.settings.continuous {
            return;
        }
        let mut scratch = std::mem::take(&mut self.continuous);
        for bullets in [false, true] {
            for index in 0..awake.ids.len() {
                let body = &awake.bodies[index];
                if !body.is_dynamic() || body.bullet != bullets {
                    continue;
                }
                let id = awake.ids[index];
                let Some(record) = records.get(id) else {
                    continue;
                };
                let Some((_, component)) = record.collider.as_ref() else {
                    continue;
                };
                let (Some(start), end) = (scratch.starts.get(index), awake.transforms[index])
                else {
                    continue;
                };
                let Some(extent) = Extent::of(component, &end) else {
                    continue;
                };
                let path = Path::new(start, &end);
                let moved = path.travel.length() + path.turn_rate * extent.core_reach;
                let fast = if bullets {
                    moved > 0.0
                } else {
                    moved >= FAST_FRACTION * extent.inner
                };
                if !fast {
                    continue;
                }
                self.counters.swept += 1;

                let bounds = Aabb::new(
                    start.position.min(end.position),
                    start.position.max(end.position),
                )
                .inflated(extent.reach);
                self.broadphase
                    .query_path(&bounds, bullets, &mut scratch.candidates);
                let lent = Bodies {
                    records,
                    statics,
                    awake,
                    islands,
                };
                let mut fraction = 1.0;
                for &proxy in &scratch.candidates {
                    if matches!(self.owners.get(proxy as usize), Some(Some(Owner::Body(other, _))) if *other == id)
                    {
                        continue;
                    }
                    let Some(obstacle) = owner_side(&self.owners, &self.planes, proxy, lent) else {
                        continue;
                    };
                    if obstacle.awake.is_some_and(|other| {
                        awake.bodies[other].bullet && awake.bodies[other].is_dynamic()
                    }) {
                        continue;
                    }
                    for part in 0..component.part_count() {
                        let Some(shape) = ContactShape::placed_part(component, part, &end) else {
                            continue;
                        };
                        self.counters.sweep_candidates += 1;
                        let spin = path.turn_rate * core_reach(&shape, end.position);
                        let place = |t: f64| {
                            ContactShape::placed_part(component, part, &path.at(t))
                                .expect("placed at the end, so placed anywhere")
                        };
                        if let Some(t) =
                            time_of_impact(&obstacle.shape, place, path.travel, spin, fraction)
                        {
                            fraction = t;
                        }
                    }
                }
                if fraction < 1.0 {
                    awake.transforms[index] = path.at(fraction);
                    self.counters.sweep_hits += 1;
                    self.counters.dropped_time += (1.0 - fraction) * dt;
                }
            }
        }
        self.continuous = scratch;
    }
}

/// Where a shape moving along a path is stopped by `obstacle`, as a share of
/// the tick before `limit`, or `None` if it is not: see the module docs.
///
/// It is stopped if its path gets a linear slop into `obstacle` — or, if it
/// began deeper than that, any deeper than it began. One that began the tick
/// clear of `obstacle` is stopped where it was a linear slop short of it; one
/// that began nearer is stopped where it got too deep.
///
/// `place` puts the moving shape a share of the way along its path, `travel`
/// is how far its body's centre goes, and `spin` the fastest its core can
/// turn, per tick.
fn time_of_impact(
    obstacle: &ContactShape,
    place: impl Fn(f64) -> ContactShape,
    travel: DVec3,
    spin: f64,
    limit: f64,
) -> Option<f64> {
    let start = gap(obstacle, &place(0.0));
    // Two tolerances below a deep start, so a body that goes no deeper is
    // never stopped where it began.
    let deep = (start.0 - 2.0 * TOLERANCE).min(-LINEAR_SLOP);
    let hit = advance(obstacle, &place, travel, spin, start, deep, limit)?;
    if start.0 > LINEAR_SLOP + TOLERANCE {
        Some(advance(obstacle, &place, travel, spin, start, LINEAR_SLOP, hit).unwrap_or(hit))
    } else {
        Some(hit)
    }
}

/// Conservative advancement from the start of the path, whose gap and normal
/// are `start`, to where the gap first comes within [`TOLERANCE`] of
/// `target`: a share of the tick before `limit`, or `None` if it does not get
/// there before `limit`.
fn advance(
    obstacle: &ContactShape,
    place: &impl Fn(f64) -> ContactShape,
    travel: DVec3,
    spin: f64,
    start: (f64, DVec3),
    target: f64,
    limit: f64,
) -> Option<f64> {
    let (mut t, (mut distance, mut normal)) = (0.0, start);
    for _ in 0..MAX_ADVANCES {
        let closing = (-travel.dot(normal)).max(0.0) + spin;
        if closing <= 0.0 {
            return None;
        }
        t += (distance - target) / closing;
        if t >= limit {
            return None;
        }
        (distance, normal) = gap(obstacle, &place(t));
        if distance <= target + TOLERANCE {
            return Some(t);
        }
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interpolation begins and ends where the body did, and its turning
    /// rate is the peak of a normalised interpolation: `4 tan(α/2)` for a
    /// turn of `2α`, a little above the turn itself.
    #[test]
    fn a_path_runs_from_start_to_end_and_bounds_its_turn() {
        let start = Transform::new(DVec3::new(1.0, 2.0, 3.0), DQuat::IDENTITY);
        let angle = 1.2;
        let end = Transform::new(
            DVec3::new(4.0, 2.0, 3.0),
            crate::rotation_from_scaled_axis(DVec3::Y * angle),
        );
        let path = Path::new(&start, &end);
        assert_eq!(path.at(0.0).position, start.position);
        assert!((path.at(1.0).position - end.position).length() < 1e-12);
        assert!(path.at(1.0).rotation.dot(end.rotation).abs() > 1.0 - 1e-12);
        let expected =
            4.0 * crcbl_core::trig::sin(angle / 4.0) / crcbl_core::trig::cos(angle / 4.0);
        assert!(
            (path.turn_rate - expected).abs() < 1e-12,
            "{}",
            path.turn_rate
        );
        assert!(path.turn_rate > angle);

        // The steepest stretch of the interpolation turns no faster than the
        // bound.
        let steps = 1000;
        let rate = (0..steps)
            .map(|k| {
                let (a, b) = (
                    path.at(f64::from(k) / f64::from(steps)).rotation,
                    path.at(f64::from(k + 1) / f64::from(steps)).rotation,
                );
                2.0 * (b * a.conjugate()).xyz().length() * f64::from(steps)
            })
            .fold(0.0, f64::max);
        assert!(rate <= path.turn_rate + 1e-6, "{rate} > {}", path.turn_rate);
        assert!(rate > 0.999 * path.turn_rate, "{rate}");
    }

    /// A ball crossing a thin plate within one step of its path meets it,
    /// just short of the surface; one heading away never does.
    #[test]
    fn a_ball_through_a_plate_is_stopped_short_of_it() {
        let plate = ContactShape::Box {
            centre: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
            half: DVec3::new(0.005, 1.0, 1.0),
        };
        let radius = 0.05;
        let start = DVec3::new(-1.0, 0.0, 0.0);
        let travel = DVec3::new(2.0, 0.0, 0.0);
        let place = |t: f64| ContactShape::Sphere {
            centre: start + travel * t,
            radius,
        };
        let t = time_of_impact(&plate, place, travel, 0.0, 1.0).expect("it crosses the plate");
        let (d, _) = gap(&plate, &place(t));
        assert!(
            (d - LINEAR_SLOP).abs() <= TOLERANCE,
            "stopped {d} m from the surface"
        );
        // One that ends the tick touching the plate, as a solve that landed it
        // there leaves it, is not stopped.
        let to_touch = DVec3::X * (-start.x - 0.005 - radius);
        let landing = |t: f64| ContactShape::Sphere {
            centre: start + to_touch * t,
            radius,
        };
        assert_eq!(time_of_impact(&plate, landing, to_touch, 0.0, 1.0), None);
        let away = |t: f64| ContactShape::Sphere {
            centre: start - travel * t,
            radius,
        };
        assert_eq!(time_of_impact(&plate, away, -travel, 0.0, 1.0), None);
        assert_eq!(time_of_impact(&plate, place, travel, 0.0, 0.3), None);
    }
}
