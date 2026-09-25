//! Moving a prone body: [`CharacterController::move_lying`].
//!
//! A lying body is a [`LyingCapsule`] whose head is the controller's
//! position. It moves the way the upright capsule does — the same plane-set
//! slide, the same skin width, the same slope limit — with two differences,
//! each forced by the shape:
//!
//! * **The whole body is swept**, not the sphere at its head, so the legs stop
//!   at a wall behind or beside the player as the head stops at one ahead. The
//!   sweep turns nothing: the yaw and pitch are what the caller and the last
//!   settle left.
//! * **It settles along its length.** Where the upright capsule probes once
//!   under its centre, a lying one probes under each end of its core and lies
//!   on the line between the two resting points, pitched to follow the ground
//!   and clamped to the walkable slope; see
//!   [`move_lying`](CharacterController::move_lying) for what happens where
//!   that line would cut through an edge.
//!
//! A lying body does not step up, and it is not dug out of what it starts
//! inside: there is no penetration depth for a lying capsule to push it out
//! by.

use glam::DVec3;

use super::{Body, CharacterController, GroundProbe, MIN_MOVE, UP};
use crate::broadphase::Segment;
use crate::collider::LyingCapsule;
use crate::world::PhysicsWorld;

/// The most poses a settle tries turning a body toward its resting line,
/// each moving an end at most a radius, before it gives up stepping and
/// bisects what is left.
const MAX_TURN_STEPS: u32 = 64;

/// The most halvings a settle spends finding where a turning body first
/// touches an edge. Each halves the gap left, so this many take any turn
/// below a floating-point step.
const MAX_BISECTIONS: u32 = 48;

/// What one [`CharacterController::move_lying`] did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LyingMoveOutcome {
    /// The body where the move left it: the head at the controller's new
    /// [`position`](CharacterController::position), the yaw it was given, and
    /// the pitch the ground gave it. **Pass this to the next move**, turned if
    /// the game has checked the turn with
    /// [`lying_blocker`](CharacterController::lying_blocker).
    pub body: LyingCapsule,
    /// How far the head moved, the settle included: the requested
    /// displacement minus whatever the world took away, as
    /// [`MoveOutcome::motion`](super::MoveOutcome::motion) is.
    pub motion: DVec3,
    /// Whether the body lies on walkable ground now. Agrees with
    /// [`CharacterController::is_grounded`].
    pub grounded: bool,
    /// Whether the move was blocked by a surface too steep to stand on and not
    /// steep enough to be a ceiling.
    pub hit_wall: bool,
    /// Whether the move was blocked by a surface facing downward.
    pub hit_ceiling: bool,
    /// How many times the move was blocked and redirected. Zero means it went
    /// the whole way unobstructed.
    pub slides: u32,
}

/// Which end of a body stays where it is while the body turns toward its
/// resting line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pivot {
    /// The head keeps its height and the feet swing: turning from the feet
    /// raised as far as the slope limit allows, down.
    Head,
    /// The feet end keeps its height and the head swings: turning from the
    /// head raised as far as the slope limit allows, down.
    Feet,
}

impl CharacterController {
    /// Move a body lying as `body` by a **world-space** displacement of its
    /// head, sliding along whatever gets in the way of any part of it, and
    /// settle it on the ground along its length.
    ///
    /// The move starts from `body`: its head becomes this controller's
    /// [`position`](Self::position) — a head somewhere else is a teleport, and
    /// forgets the ground as [`set_position`](Self::set_position) does — and
    /// its yaw, pitch, radius and length are the shape swept. The controller's
    /// own [`config`](Self::config) supplies the rest: the skin width, the
    /// slope limit, the slide budget and the snap distance, but not the shape.
    /// The [self collider](Self::with_self_collider) and the
    /// [query mask](Self::with_query_mask) apply as they do to every other
    /// query, and the self collider is written back as this controller's own
    /// [`capsule`](Self::capsule) at the head, as a walking move writes it.
    ///
    /// # The move
    ///
    /// `motion` is a displacement and not a velocity, and a grounded move
    /// replaces its vertical part with the rise of the ground under the body,
    /// exactly as [`move_and_slide`](Self::move_and_slide) does. The whole
    /// capsule is swept along it, and each surface it meets is slid along with
    /// the same plane-set clip. **The body does not turn while it moves**: a
    /// change of yaw is the caller's, checked beforehand with
    /// [`lying_blocker`](Self::lying_blocker), and a change of pitch is the
    /// settle's.
    ///
    /// A lying body does not step up. A riser low enough that the round end
    /// meets its edge on a walkable slope — below about
    /// `radius · (1 - min_ground_normal_y)` plus a skin width — is slid up and
    /// over, as a ramp is. A taller one is a wall, and stays one: a grounded
    /// body meets a wall as though it were upright, as Unreal's
    /// `SlideAlongSurface` does, so the edge's upward lean does not lift it,
    /// and the settle never lifts an end onto what it cannot stand on.
    ///
    /// # The settle
    ///
    /// A sphere of the body's radius is swept straight down under each end of
    /// the core, and each end's **resting point** is where that sphere would
    /// sit a [`skin_width`](super::CharacterConfig::skin_width) above what it
    /// met. The body settles only if one end is within the walking move's own
    /// ground probe of what is under it; otherwise it is in the air, and falls
    /// as the caller moves it. Each end looks further than that, as far as the
    /// slope limit could swing it down with the other end held, so a body lying
    /// level along a slope finds the ground under its downhill end.
    ///
    /// The body lies on the line through the two resting points: its pitch is
    /// that line's, **clamped to the steepest walkable slope** — the pitch
    /// sine never exceeds `sqrt(1 - min_ground_normal_y²)` either way — and it
    /// is placed as low as it can go while neither end sinks below its
    /// resting point. On even ground within the limit both ends rest; on
    /// ground steeper than it, the uphill end rests and the downhill end is
    /// held off the slope. An end with nothing under it as far as it looks
    /// swings down to the limit, the body resting on the other end — or, where
    /// that swings it into the ground, turning onto it as an edge is met
    /// below.
    ///
    /// **A line that would cut through an edge** — a curb the head has gone
    /// over, the crest of a hill, a ridge under the middle — is refused. The
    /// body turns toward it instead about one end, from that end's other end
    /// raised as far as the slope limit allows down to where it first touches
    /// what is in the way; about the head and about the feet are both tried,
    /// and the one that turns less wins, the head on a tie. Crawling head
    /// first off a curb, the body therefore rides level with its head out over
    /// the drop, then tips head down once resting on the head is the smaller
    /// turn. If neither turn has a clear pose to start from, the body is left
    /// as the slide left it.
    ///
    /// The ground [`ground`](Self::ground) reports is the head's, or the feet
    /// end's where the head's is not walkable. The body is grounded when
    /// either is.
    pub fn move_lying(
        &mut self,
        world: &mut PhysicsWorld,
        body: &LyingCapsule,
        motion: DVec3,
    ) -> LyingMoveOutcome {
        if body.head != self.position {
            self.set_position(body.head);
        }
        let start = self.position;
        let was_grounded = self.ground.is_some();

        let motion = self.ground_adjusted(motion, was_grounded);
        let report = self.slide(world, motion, was_grounded, Body::Lying(*body));
        let slid = LyingCapsule {
            head: self.position,
            ..*body
        };
        let settled = self.settle_lying(world, slid, self.settle_reach(was_grounded, motion));
        self.position = settled.head;

        if let Some(collider) = self.self_collider {
            world.set_capsule(collider, self.capsule());
        }

        LyingMoveOutcome {
            body: settled,
            motion: self.position - start,
            grounded: self.ground.is_some(),
            hit_wall: report.hit_wall,
            hit_ceiling: report.hit_ceiling,
            slides: report.slides,
        }
    }

    /// Lay `body` on the ground under its two ends, looking `reach` down: see
    /// [`move_lying`](Self::move_lying)'s "The settle".
    fn settle_lying(
        &mut self,
        world: &mut PhysicsWorld,
        body: LyingCapsule,
        reach: f64,
    ) -> LyingCapsule {
        // Each end looks further than `reach` by as far as the pitch could
        // still lower it with the other end held — so a body lying level on a
        // slope finds the ground under its downhill end — but the body only
        // settles if one end is within `reach`: a body with both ends further
        // off than that is in the air, and falls as the caller moves it.
        let feet = body.feet();
        let limit = self.steepest_pitch_sine();
        let swing = |sine: f64| reach + body.length * sine.max(0.0);
        let under_head = self.probe_end(
            world,
            body.head,
            body.radius,
            swing(limit + body.pitch_sine),
        );
        let under_feet = self.probe_end(world, feet, body.radius, swing(limit - body.pitch_sine));
        self.ground = None;
        let near = |probe: &GroundProbe| probe.distance <= reach;
        if !under_head.iter().chain(&under_feet).any(near) {
            return body;
        }
        self.ground = under_head
            .filter(|probe| probe.walkable)
            .or(under_feet.filter(|probe| probe.walkable))
            .map(|probe| probe.contact);

        // Signed, as the walking settle's is: an end nearer walkable ground
        // than a skin width is lifted back to it, by no more than that width.
        // Nothing lifts an end off what it cannot stand on, which is how a
        // body pressed against the edge of a riser would otherwise climb it a
        // skin width a tick.
        let skin = self.config.skin_width;
        let rest = |end: DVec3, probe: GroundProbe| {
            let lift = if probe.walkable { -skin } else { 0.0 };
            end.y - (probe.distance - skin).max(lift)
        };
        let level = body.length * (1.0 - body.pitch_sine * body.pitch_sine).max(0.0).sqrt();
        // An end with nothing under it as far as it looks swings to the limit.
        let pitch_sine = match (under_head, under_feet) {
            (None, None) => return body,
            _ if level <= MIN_MOVE => body.pitch_sine,
            (Some(head), Some(feet_probe)) => {
                self.resting_pitch(rest(body.head, head), rest(feet, feet_probe), level)
            }
            (Some(_), None) => limit,
            (None, Some(_)) => -limit,
        };
        // The pitch turns the body about its head, so the feet end moves
        // along the ground from where it was probed: its resting height is
        // carried there along the plane it met, exact on a plane. The body is
        // then as low as it can lie with neither end below its resting height.
        let turned = LyingCapsule { pitch_sine, ..body };
        let rise = turned.head.y - turned.feet().y;
        let shift = turned.feet() - feet;
        let on_head = under_head.map(|probe| rest(body.head, probe));
        let on_feet = under_feet.map(|probe| {
            let normal = probe.contact.normal;
            let along = if normal.y > MIN_MOVE {
                (normal.x * shift.x + normal.z * shift.z) / normal.y
            } else {
                0.0
            };
            rest(feet, probe) - along + rise
        });
        let height = on_head
            .into_iter()
            .chain(on_feet)
            .fold(f64::NEG_INFINITY, f64::max);
        let resting = LyingCapsule {
            head: DVec3::new(body.head.x, height, body.head.z),
            ..turned
        };
        if self.lying_blocker(world, &resting).is_none() {
            return resting;
        }

        let about_head = self.turn_to_rest(world, &resting, Pivot::Head);
        let about_feet = self.turn_to_rest(world, &resting, Pivot::Feet);
        let turn = |pose: &LyingCapsule| (pose.pitch_sine - resting.pitch_sine).abs();
        match (about_head, about_feet) {
            (Some(head), Some(feet)) if turn(&feet) < turn(&head) => feet,
            (Some(head), _) => head,
            (None, Some(feet)) => feet,
            (None, None) => body,
        }
    }

    /// The pitch sine of the line through two ends' resting heights `level`
    /// apart horizontally, clamped to the slope limit.
    fn resting_pitch(&self, head_rest: f64, feet_rest: f64, level: f64) -> f64 {
        let limit = self.steepest_pitch_sine();
        let gradient = (head_rest - feet_rest) / level;
        (gradient / (1.0 + gradient * gradient).sqrt()).clamp(-limit, limit)
    }

    /// The largest pitch sine a lying body takes, either way: the sine of the
    /// steepest walkable slope, whose cosine is
    /// [`min_ground_normal_y`](super::CharacterConfig::min_ground_normal_y).
    fn steepest_pitch_sine(&self) -> f64 {
        let cosine = self.config.min_ground_normal_y.clamp(0.0, 1.0);
        (1.0 - cosine * cosine).sqrt()
    }

    /// `resting`, which is blocked, turned about `pivot` until it is clear:
    /// from the other end raised as far as the slope limit allows, down, to
    /// the last pose short of touching. `None` if even that first pose is
    /// blocked, or there is no turn to make.
    ///
    /// The turn is stepped so no end moves more than a radius from one pose to
    /// the next — so nothing thinner than the body is stepped over — then the
    /// first blocked step is bisected until the swinging end's travel is below
    /// half a skin width.
    fn turn_to_rest(
        &self,
        world: &mut PhysicsWorld,
        resting: &LyingCapsule,
        pivot: Pivot,
    ) -> Option<LyingCapsule> {
        let limit = self.steepest_pitch_sine();
        let feet_height = resting.feet().y;
        let pose = |pitch_sine: f64| {
            let head_height = match pivot {
                Pivot::Head => resting.head.y,
                Pivot::Feet => feet_height + resting.length * pitch_sine,
            };
            LyingCapsule {
                head: DVec3::new(resting.head.x, head_height, resting.head.z),
                pitch_sine,
                ..*resting
            }
        };
        let top = match pivot {
            Pivot::Head => -limit,
            Pivot::Feet => limit,
        };
        let span = resting.pitch_sine - top;
        if span.abs() <= MIN_MOVE || self.lying_blocker(world, &pose(top)).is_some() {
            return None;
        }

        // An end swings `length · Δθ`, and `Δθ ≤ Δsine / cos θ`, where the
        // cosine is least at the steeper of the two ends of the turn.
        let steepest = top.abs().max(resting.pitch_sine.abs());
        let cosine = (1.0 - steepest * steepest).sqrt();
        let steps = (resting.length * span.abs() / (resting.radius * cosine))
            .ceil()
            .clamp(1.0, f64::from(MAX_TURN_STEPS));
        let (mut clear, mut blocked) = (top, None);
        for step in 1..=MAX_TURN_STEPS {
            let share = f64::from(step) / steps;
            if share > 1.0 {
                break;
            }
            let sine = top + span * share;
            if self.lying_blocker(world, &pose(sine)).is_some() {
                blocked = Some(sine);
                break;
            }
            clear = sine;
        }
        let Some(mut blocked) = blocked else {
            return Some(pose(clear));
        };
        for _ in 0..MAX_BISECTIONS {
            if resting.length * (blocked - clear).abs() <= 0.5 * self.config.skin_width {
                break;
            }
            let middle = 0.5 * (clear + blocked);
            if self.lying_blocker(world, &pose(middle)).is_some() {
                blocked = middle;
            } else {
                clear = middle;
            }
        }
        Some(pose(clear))
    }

    /// Sweep a sphere of `radius` straight down `distance` from `centre`, as
    /// the walking probe sweeps the capsule, and describe what it meets.
    fn probe_end(
        &self,
        world: &mut PhysicsWorld,
        centre: DVec3,
        radius: f64,
        distance: f64,
    ) -> Option<GroundProbe> {
        let found = world.sweep_sphere_filtered(
            &Segment::new(centre, centre - UP * distance),
            radius,
            self.filter(),
        )?;
        Some(self.ground_probe(found, distance))
    }
}
