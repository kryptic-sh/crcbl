//! Capsule character controller: sweep-based movement against the world.
//!
//! [`CharacterController`] moves a Y-aligned capsule through a
//! [`PhysicsWorld`], sliding along what it hits, walking up slopes it is
//! allowed to and over steps it can reach, staying attached to the ground it
//! walks off the edge of, and pushing itself out of anything it wakes up
//! inside. It is kinematic: nothing integrates it, and it moves exactly as far
//! as the caller asks minus what the world takes away.
//!
//! # It does not know which camera is watching
//!
//! [`CharacterController::move_and_slide`] takes a **world-space
//! displacement** and nothing else. It holds no camera, no view basis and no
//! yaw, and it never derives a direction from one. Turning a stick vector into
//! a world direction is the caller's job, because that is the only step that
//! genuinely differs between a first-person rig (intent is relative to the
//! view) and a third-person one (intent is relative to the camera while the
//! body turns toward it over time).
//!
//! **Facing is not here either.** There is no orientation on this type. A
//! first-person game pins the character's facing to its view; a third-person
//! game turns the body toward the direction it is moving, over time, and reads
//! that direction from [`MoveOutcome::motion`]. A controller that stored one
//! yaw and used it for both could not do the second without fighting the first,
//! which is where the two styles usually get forked into two controllers.
//!
//! Camera collision, follow logic and spring arms are the camera's, and
//! `docs/plan/30-player-kit.md` puts those on the client as presentation while
//! movement runs as a server system.
//!
//! Jump buffering, coyote time, sprint multipliers, acceleration curves and
//! input mapping are not here either. They are a layer above this one, and they
//! reach it through the displacement they ask for.
//!
//! # The approach, and where it comes from
//!
//! The move is a **collect-and-slide loop over a plane set**, which is the
//! shape every engine converges on:
//!
//! * Quake's `SV_FlyMove` / `PM_SlideMove` sweeps, records each blocking
//!   plane, and clips the velocity against the *set* rather than the last one —
//!   picking the plane whose clipped result does not drive into any other, and
//!   falling back to the crease (the cross product of two planes) when no
//!   single plane frees it. It stops dead when the clipped velocity opposes the
//!   original, which is what keeps a sloping corner from oscillating. That
//!   loop, its bump count ([`CharacterConfig::max_slides`]) and its plane
//!   budget ([`MAX_PLANES`]) are reproduced here.
//! * Godot's `CharacterBody3D::move_and_slide` contributes the iteration cap as
//!   a tunable, the walkable-floor angle as the one thing that decides what
//!   ground is, and `floor_snap_length` — staying attached to a surface that
//!   drops away under you.
//! * Unreal's `UCharacterMovementComponent` contributes the two pieces Quake
//!   has no equivalent of: `StepUp`'s rise / advance / drop with a *validated*
//!   landing, and `ComputeGroundMovementDelta`'s rule that a grounded move
//!   keeps its horizontal length and takes its rise from the ramp — so walking
//!   up a slope is not slower than walking on the flat.
//!
//! One deliberate difference from the Quake lineage: its `ClipVelocity` takes
//! an `overbounce` factor, which Quake III passes slightly above one to nudge
//! off the plane it just clipped against. This backs the capsule off by
//! [`CharacterConfig::skin_width`] instead. The gap is then a distance the
//! reader can name, and the next sweep starts outside the surface rather than
//! exactly on it.
//!
//! # Failure modes
//!
//! Handled, each with the mechanism that handles it:
//!
//! * **A crease between two planes sliding you into geometry.** Clipping
//!   against the plane *set* and falling back to the crease direction, from
//!   `SV_FlyMove`. A third simultaneous plane is a corner and stops the
//!   character rather than squeezing it through.
//! * **Oscillating in a sloping corner.** The clipped displacement is dropped
//!   when it opposes the one the move started with.
//! * **Creeping down a slope while standing still.** A grounded move takes its
//!   rise from the ramp and *discards* the vertical it was asked for rather
//!   than clipping it against the slope, so a request with no horizontal part
//!   becomes no movement at all. Godot spends `floor_stop_on_slope` on the same
//!   problem. The ground probe is what holds the character on.
//! * **Climbing a wall by facing into a corner.** Three things refuse it: the
//!   step-up needs the advance after rising to make real progress, it needs the
//!   drop afterwards to land on walkable ground, and it may happen at most once
//!   per call so a character cannot ratchet up a wall within one tick. The
//!   ground probe is a fourth — it pulls the capsule back down onto whatever it
//!   is actually standing on.
//! * **Tunnelling at speed.** Every probe is a swept capsule, never a
//!   teleport-and-test.
//! * **Waking up inside geometry.** [`Penetration`] depths, resolved deepest
//!   first before the move.
//!
//! Not handled, and known:
//!
//! * **A step-up that clips the wall it stepped over.** The advance is swept at
//!   the raised height and the drop is swept straight down, but the corner
//!   between them is not. A capsule that lands touching the riser is pushed out
//!   by the next call's depenetration rather than being placed clear by this
//!   one.
//! * **Moving geometry.** Nothing here reads a collider's velocity, so a
//!   character standing on a platform does not ride it and a platform closing
//!   on a character resolves as a penetration rather than as a push.
//! * **Ceilings while stepping up.** The rise stops under an overhead
//!   obstruction, so a low ceiling shortens the step rather than cancelling it;
//!   whether the shortened step then lands is left to the same landing check as
//!   any other.
//! * **More than two simultaneous planes.** Stopping dead is Quake's answer and
//!   it is this one; Quake III tries the remaining pairs before giving up.

use glam::DVec3;

use crate::broadphase::Segment;
use crate::collider::Capsule;
use crate::query::{Penetration, ShapeHit};
use crate::world::{ColliderId, PhysicsWorld};

/// The world's up axis. `crcbl` is right-handed with `+Y` up, and
/// [`Capsule`] is Y-aligned, so a character controller has exactly one.
const UP: DVec3 = DVec3::Y;

/// A displacement shorter than this is not worth a sweep: the capsule is
/// already where it was going, and sweeping it would only cost a broadphase
/// traversal to be told so.
const MIN_MOVE: f64 = 1e-9;

/// How many blocking planes one move may collect before the character is
/// declared stuck and stopped.
///
/// Quake's `MAX_CLIP_PLANES`, and reaching it is its "this shouldn't really
/// happen": a character that has found this many distinct surfaces without
/// covering any ground is wedged rather than on a path. It is a different limit
/// from the one `clip_to_planes` enforces, which is that only two planes have a
/// crease to run along.
pub const MAX_PLANES: usize = 5;

/// How many [`CharacterConfig::skin_width`]s of slack a downward probe carries
/// beyond the distance it is actually asking about.
///
/// A settled capsule sits exactly one skin width of *vertical* travel above its
/// ground, so a probe with no slack would find that ground only by touching it,
/// and a sweep that ends exactly at contact is a knife edge. Two skin widths is
/// the shortest slack that clears it. Unreal spends its `MAX_FLOOR_DIST` the
/// same way.
const GROUND_PROBE_SKINS: f64 = 2.0;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// The shape of a character and the limits it moves under.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharacterConfig {
    /// Radius of the capsule.
    pub radius: f64,
    /// Half the length of the capsule's cylindrical section, as
    /// [`Capsule::half_height`] means it: the standing height is
    /// `2 * (half_height + radius)`.
    pub half_height: f64,
    /// The smallest `normal · +Y` a surface can have and still be ground the
    /// character stands and walks on — the **cosine** of the steepest walkable
    /// slope, not the angle.
    ///
    /// A cosine and not an angle because this crate is a determinism-bearing
    /// one and `cos` is a platform transcendental: see the correction in
    /// `docs/plan/05-physics.md`. Authoring an angle is the caller's job, done
    /// once, outside the tick. This is the same runtime form Quake's
    /// `normal[2] > 0.7` and Unreal's `WalkableFloorZ` keep.
    ///
    /// Where the behaviour actually flips is measured by
    /// `tests::the_slope_the_controller_stops_walking_up_is_the_one_it_was_configured_with`.
    pub min_ground_normal_y: f64,
    /// The tallest ledge the character walks up without leaving the ground.
    ///
    /// Also how far it will drop to stay attached to ground that falls away
    /// under it, which is Godot's `floor_snap_length` and the reason walking
    /// down stairs does not become a series of falls.
    ///
    /// The rise is taken with a [`skin_width`](Self::skin_width) to spare, so a
    /// step of exactly this height is climbed and the true cut-off is up to one
    /// skin width above it. The band is measured by
    /// `tests::the_step_the_controller_stops_climbing_is_the_offset_it_was_configured_with`.
    pub step_offset: f64,
    /// The gap kept between the capsule and every surface it touches.
    ///
    /// Every sweep stops this far short of its contact, so the next sweep
    /// starts outside the surface instead of exactly on it, where a touching
    /// start would be reported as an overlap. Unity's `CharacterController`
    /// calls the same quantity `skinWidth`; Unreal spends it as
    /// `MAX_FLOOR_DIST`.
    pub skin_width: f64,
    /// How many times one move may be clipped and redirected before it stops.
    ///
    /// Quake's `numbumps` and Godot's `max_slides`.
    pub max_slides: u32,
    /// How many times the move may push the capsule out of what it overlaps
    /// before giving up and moving anyway.
    ///
    /// One pass resolves one contact completely, so this is the number of
    /// *distinct* surfaces a character can be dug out of in a single call — a
    /// corner needs two, a hole in a wall three.
    pub depenetration_passes: u32,
}

impl Default for CharacterConfig {
    /// An adult-sized character: 1.8 m tall, 0.6 m across, 45° slopes.
    ///
    /// 45° is expressed as [`std::f64::consts::FRAC_1_SQRT_2`] rather than
    /// `(PI / 4.0).cos()` so the default costs no transcendental at all — see
    /// [`min_ground_normal_y`](Self::min_ground_normal_y).
    fn default() -> Self {
        Self {
            radius: 0.3,
            half_height: 0.6,
            min_ground_normal_y: std::f64::consts::FRAC_1_SQRT_2,
            step_offset: 0.4,
            skin_width: 0.01,
            max_slides: 4,
            depenetration_passes: 4,
        }
    }
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// The ground a character is standing on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundContact {
    /// Upward unit normal of the surface.
    pub normal: DVec3,
    /// Where the capsule meets it.
    pub point: DVec3,
    /// Which collider it is.
    pub collider: ColliderId,
}

/// What one [`CharacterController::move_and_slide`] did.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MoveOutcome {
    /// The displacement actually applied, which is the requested one minus
    /// whatever the world took away. **This is what a third-person game turns
    /// the body toward**, and it is world-space for the same reason the
    /// request is.
    pub motion: DVec3,
    /// How far the capsule was pushed out of geometry it started inside,
    /// before the move. Zero in the ordinary case.
    pub depenetration: DVec3,
    /// Whether the character is standing on walkable ground now. Agrees with
    /// [`CharacterController::is_grounded`].
    pub grounded: bool,
    /// Whether the move was blocked by a surface too steep to stand on and not
    /// steep enough to be a ceiling.
    pub hit_wall: bool,
    /// Whether the move was blocked by a surface facing downward.
    pub hit_ceiling: bool,
    /// Whether a step was climbed.
    pub stepped_up: bool,
    /// How many times the move was blocked and redirected. Zero means it went
    /// the whole way unobstructed.
    pub slides: u32,
}

/// What the slide loop found, before the ground probe has its say.
#[derive(Debug, Clone, Copy, Default)]
struct SlideReport {
    hit_wall: bool,
    hit_ceiling: bool,
    stepped_up: bool,
    slides: u32,
}

/// A step-up that survived all three of its checks.
#[derive(Debug, Clone, Copy)]
struct StepUp {
    /// Where the capsule ends up.
    position: DVec3,
    /// The horizontal displacement the advance consumed.
    advance: DVec3,
}

// ---------------------------------------------------------------------------
// Controller
// ---------------------------------------------------------------------------

/// A capsule that walks.
///
/// Drive it one displacement at a time, at the fixed server timestep:
///
/// ```
/// use crcbl_phys::{BoxCollider, CharacterConfig, CharacterController, PhysicsWorld};
/// use glam::DVec3;
///
/// let mut world = PhysicsWorld::new();
/// world.add_box(BoxCollider::new(
///     DVec3::new(0.0, -1.0, 0.0),
///     DVec3::new(50.0, 1.0, 50.0),
/// ));
///
/// let config = CharacterConfig::default();
/// // Feet on the floor: the capsule centre is a radius plus a half-height up.
/// let feet = config.radius + config.half_height;
/// let mut character = CharacterController::new(config, DVec3::new(0.0, feet, 0.0));
///
/// let dt = 1.0 / 60.0;
/// // The caller owns the camera, so the caller owns this direction.
/// let intent = DVec3::new(1.0, 0.0, 0.0);
/// let gravity = DVec3::new(0.0, -9.81 * dt * dt, 0.0);
///
/// for _ in 0..30 {
///     let outcome = character.move_and_slide(&mut world, intent * 4.0 * dt + gravity);
///     assert!(outcome.grounded);
/// }
/// assert!(character.position().x > 1.0);
/// ```
#[derive(Debug, Clone)]
pub struct CharacterController {
    config: CharacterConfig,
    position: DVec3,
    ground: Option<GroundContact>,
    self_collider: Option<ColliderId>,
    /// Kept across calls so depenetration allocates nothing per tick.
    contacts: Vec<(ColliderId, Penetration)>,
}

impl CharacterController {
    /// Place a character at `position`, which is the **centre** of its capsule
    /// — a character whose feet are on `y = 0` starts at
    /// `radius + half_height`.
    ///
    /// It starts ungrounded whatever is beneath it; the first
    /// [`move_and_slide`](Self::move_and_slide), even with a zero
    /// displacement, is what finds the floor.
    #[must_use]
    pub fn new(config: CharacterConfig, position: DVec3) -> Self {
        Self {
            config,
            position,
            ground: None,
            self_collider: None,
            contacts: Vec::new(),
        }
    }

    /// Tell the controller which collider in the world is its own body, so its
    /// sweeps do not find it.
    ///
    /// A character that other things have to collide with is registered in the
    /// world like anything else, and then sits on the start of every segment it
    /// sweeps — see [`PhysicsWorld::sweep_capsule_excluding`]. Given this, each
    /// move also writes the capsule's new position back to that collider, so
    /// the world's copy is never a tick behind.
    ///
    /// A character nothing else collides with does not need it.
    #[must_use]
    pub fn with_self_collider(mut self, collider: ColliderId) -> Self {
        self.self_collider = Some(collider);
        self
    }

    /// The centre of the capsule.
    #[inline]
    #[must_use]
    pub fn position(&self) -> DVec3 {
        self.position
    }

    /// Teleport. Discards the ground, because the ground under the old position
    /// says nothing about the new one; the next move finds it again.
    pub fn set_position(&mut self, position: DVec3) {
        self.position = position;
        self.ground = None;
    }

    /// The capsule as the world sees it.
    #[inline]
    #[must_use]
    pub fn capsule(&self) -> Capsule {
        Capsule::new(self.position, self.config.radius, self.config.half_height)
    }

    /// The limits this character moves under.
    #[inline]
    #[must_use]
    pub fn config(&self) -> &CharacterConfig {
        &self.config
    }

    /// The ground the character is standing on, if it is standing on any.
    #[inline]
    #[must_use]
    pub fn ground(&self) -> Option<&GroundContact> {
        self.ground.as_ref()
    }

    /// Whether the character is standing on walkable ground.
    #[inline]
    #[must_use]
    pub fn is_grounded(&self) -> bool {
        self.ground.is_some()
    }

    /// Whether a surface with this normal is ground the character can stand on.
    ///
    /// Upward-facing is part of the question and not an assumption: a
    /// [`min_ground_normal_y`](CharacterConfig::min_ground_normal_y) of zero
    /// would otherwise make a vertical wall walkable, and then the ramp
    /// adjustment would divide by its zero rise.
    #[inline]
    #[must_use]
    pub fn is_walkable(&self, normal: DVec3) -> bool {
        let rise = normal.dot(UP);
        rise > 0.0 && rise >= self.config.min_ground_normal_y
    }

    /// Whether a surface with this normal faces down enough to be a ceiling.
    ///
    /// The mirror of [`is_walkable`](Self::is_walkable) through the horizontal,
    /// so every normal is ground, ceiling or wall and nothing is two of them.
    #[inline]
    #[must_use]
    pub fn is_ceiling(&self, normal: DVec3) -> bool {
        let rise = normal.dot(UP);
        rise < 0.0 && -rise >= self.config.min_ground_normal_y
    }

    /// Move by a **world-space** displacement, sliding along whatever gets in
    /// the way.
    ///
    /// `motion` is a displacement for this tick and not a velocity: the caller
    /// has already multiplied by its own timestep, so nothing here reads a
    /// clock and two callers stepping at different rates get the same answer
    /// for the same displacement.
    ///
    /// # What a grounded move does with the vertical part
    ///
    /// While the character is standing on walkable ground and `motion` does not
    /// point upward, the vertical part of `motion` is **replaced** by the rise
    /// or fall the ground itself imposes — Unreal's
    /// `ComputeGroundMovementDelta`. Walking up a ramp therefore covers the
    /// same horizontal distance as walking on the flat, and the gravity a
    /// caller keeps applying does not dig the capsule into the slope. What
    /// holds the character down instead is the ground probe at the end of the
    /// move.
    ///
    /// The moment the ground stops being walkable, or the caller asks to go
    /// up, `motion` is used as given and gravity does what gravity does.
    pub fn move_and_slide(&mut self, world: &mut PhysicsWorld, motion: DVec3) -> MoveOutcome {
        let depenetration = self.depenetrate(world);
        let start = self.position;
        let was_grounded = self.ground.is_some();

        let motion = self.ground_adjusted(motion, was_grounded);
        let report = self.slide(world, motion, was_grounded);
        self.settle_on_ground(world, was_grounded, motion);

        if let Some(collider) = self.self_collider {
            world.set_capsule(collider, self.capsule());
        }

        MoveOutcome {
            motion: self.position - start,
            depenetration,
            grounded: self.ground.is_some(),
            hit_wall: report.hit_wall,
            hit_ceiling: report.hit_ceiling,
            stepped_up: report.stepped_up,
            slides: report.slides,
        }
    }

    // ── Phases ─────────────────────────────────────────────────────────

    /// Push the capsule out of everything it overlaps, deepest contact first.
    ///
    /// One contact per pass, rather than the sum of all of them: two surfaces
    /// sharing a normal would double the push, and a corner's two normals
    /// summed points somewhere neither of them does. Resolving the deepest and
    /// asking again converges on the corner and never overshoots.
    fn depenetrate(&mut self, world: &mut PhysicsWorld) -> DVec3 {
        let mut total = DVec3::ZERO;
        for _ in 0..self.config.depenetration_passes {
            let capsule = self.capsule();
            let mut contacts = std::mem::take(&mut self.contacts);
            world.capsule_penetrations_into(&capsule, self.self_collider, &mut contacts);
            let deepest = contacts
                .iter()
                .max_by(|a, b| a.1.depth.total_cmp(&b.1.depth))
                .map(|&(_, penetration)| penetration);
            self.contacts = contacts;

            let Some(penetration) = deepest else {
                break;
            };
            let push = penetration.normal * (penetration.depth + self.config.skin_width);
            self.position += push;
            total += push;
        }
        total
    }

    /// Turn a requested displacement into the one a grounded character
    /// actually makes.
    ///
    /// While the character is on walkable ground and not asking to leave it,
    /// the horizontal part of the request keeps its length and takes its rise
    /// from the ground plane — Unreal's `ComputeGroundMovementDelta` with
    /// `bMaintainHorizontalGroundVelocity`. The vertical part is **discarded**,
    /// not projected.
    ///
    /// Discarding it is what stops a standing character creeping downhill.
    /// Clipping gravity against the slope instead, which is what happens the
    /// moment this does not run, leaves a downhill component behind on every
    /// tick, and Godot spends `floor_stop_on_slope` on the same problem. It
    /// also means a request with no horizontal part becomes no movement at all,
    /// because the ramp form of a zero horizontal is zero.
    fn ground_adjusted(&self, motion: DVec3, was_grounded: bool) -> DVec3 {
        if !was_grounded || motion.dot(UP) > 0.0 {
            return motion;
        }
        let Some(ground) = self.ground else {
            return motion;
        };
        let flat = motion - UP * motion.dot(UP);
        flat - UP * (ground.normal.dot(flat) / ground.normal.dot(UP))
    }

    /// The collect-and-slide loop, after `SV_FlyMove`.
    fn slide(
        &mut self,
        world: &mut PhysicsWorld,
        motion: DVec3,
        was_grounded: bool,
    ) -> SlideReport {
        let mut report = SlideReport::default();
        let primal = motion;
        let mut remaining = motion;
        // Quake's `original_velocity`: what the plane set is asked to clip.
        // It is only re-read once the capsule has actually covered ground,
        // because a plane found without moving has to be added to the ones
        // already there rather than replacing them.
        let mut clip_from = motion;
        let mut planes = [DVec3::ZERO; MAX_PLANES];
        let mut plane_count = 0usize;

        for _ in 0..self.config.max_slides {
            let distance = remaining.length();
            if distance <= MIN_MOVE {
                return report;
            }
            let direction = remaining / distance;
            let target = self.position + remaining;

            let Some((_, hit)) = self.sweep(world, self.position, target) else {
                self.position = target;
                return report;
            };
            report.slides += 1;

            let travel = (hit.t * distance - self.config.skin_width).clamp(0.0, distance);
            if travel > 0.0 {
                let step = direction * travel;
                self.position += step;
                remaining -= step;
                clip_from = remaining;
                plane_count = 0;
            } else if hit.started_inside {
                // The sweep began *on* the surface rather than short of it, so
                // clipping alone would leave the next sweep starting on it too
                // and reporting the same contact for ever. A capsule set down
                // exactly on the floor is the ordinary way to arrive here, and
                // it must still be able to walk. Back off by the gap every
                // other sweep already keeps.
                self.position += hit.normal * self.config.skin_width;
            }

            if self.is_ceiling(hit.normal) {
                report.hit_ceiling = true;
            } else if !self.is_walkable(hit.normal) {
                report.hit_wall = true;
                if was_grounded
                    && !report.stepped_up
                    && let Some(step) = self.try_step_up(world, remaining)
                {
                    self.position = step.position;
                    remaining -= step.advance;
                    clip_from = remaining;
                    plane_count = 0;
                    report.stepped_up = true;
                    continue;
                }
            }

            if plane_count == MAX_PLANES {
                return report;
            }
            planes[plane_count] = hit.normal;
            plane_count += 1;

            remaining = clip_to_planes(clip_from, &planes[..plane_count], remaining);
            if remaining.dot(primal) <= 0.0 {
                return report;
            }
        }
        report
    }

    /// Rise, advance, drop — Unreal's `StepUp`, with its landing check.
    ///
    /// The three refusals stop three different things. The **advance** check is
    /// what makes a corner not a step: after rising, a corner is still a wall
    /// in both directions and nothing was stepped onto. The **landing** check
    /// keeps the capsule off thin air when the drop finds nothing under it. The
    /// **walkable** test keeps it off a surface it is not allowed to stand on
    /// and would slide straight back off.
    ///
    /// They are not the last line: the ground probe that ends every move pulls
    /// the capsule back down onto whatever it is really standing on, so a step
    /// that rose without earning it is undone anyway. All four have to be gone
    /// at once before a character in a corner actually gains height, which is
    /// what `tests::a_character_pressed_into_a_corner_does_not_climb_it` was
    /// checked against.
    fn try_step_up(&self, world: &mut PhysicsWorld, remaining: DVec3) -> Option<StepUp> {
        if self.config.step_offset <= 0.0 {
            return None;
        }
        let forward = remaining - UP * remaining.dot(UP);
        let forward_distance = forward.length();
        if forward_distance <= MIN_MOVE {
            return None;
        }
        let forward_direction = forward / forward_distance;

        // Rise. A settled capsule is already a skin width off its floor, and
        // that gap is what lets a step of exactly `step_offset` clear its own
        // top rather than starting the advance in contact with it. An overhead
        // obstruction shortens the rise instead of cancelling it.
        let rise = self.clear_travel(world, self.position, UP * self.config.step_offset);
        if rise <= MIN_MOVE {
            return None;
        }
        let raised = self.position + UP * rise;

        // Advance. No progress up here means this is a wall and not a step.
        let advance = self.clear_travel(world, raised, forward);
        if advance <= MIN_MOVE {
            return None;
        }
        let over = raised + forward_direction * advance;

        // Drop, far enough to actually reach what it is landing on: the rise
        // it is undoing plus the gap it started from, or a step of no height at
        // all comes back as a sweep that ends exactly at contact and is
        // reported as a miss. Unreal spends the same two floor distances on it.
        let drop = rise + self.config.skin_width * GROUND_PROBE_SKINS;
        let (_, landing) = self.sweep(world, over, over - UP * drop)?;
        if landing.started_inside || !self.is_walkable(landing.normal) {
            return None;
        }
        let fall = (landing.t * drop - self.config.skin_width).clamp(0.0, drop);

        Some(StepUp {
            position: over - UP * fall,
            advance: forward_direction * advance,
        })
    }

    /// Find the ground, and drop onto it if it fell away under the character.
    ///
    /// The probe is what makes the ground authoritative: a walkable plane the
    /// slide loop happened to graze is not standing on it, and a character that
    /// walked off a lip is still standing on the floor below. Snapping only
    /// happens to a character that was already walking and is not asking to go
    /// up, so a jump is not swallowed by the floor it just left.
    fn settle_on_ground(&mut self, world: &mut PhysicsWorld, was_grounded: bool, motion: DVec3) {
        self.ground = None;
        let snap = if was_grounded && motion.dot(UP) <= 0.0 {
            self.config.step_offset
        } else {
            0.0
        };
        let probe = snap + self.config.skin_width * GROUND_PROBE_SKINS;

        let Some((collider, hit)) = self.sweep(world, self.position, self.position - UP * probe)
        else {
            return;
        };
        if !self.is_walkable(hit.normal) {
            return;
        }
        // Signed, and so it corrects in both directions: a capsule that ended
        // the move *closer* to the floor than a skin width — set down on it, or
        // walked onto it — is lifted back off, which is what keeps the next
        // horizontal sweep from starting on the floor's surface and being told
        // it is already in contact. Unreal spends `AdjustFloorHeight` on the
        // same correction. The lift is capped at the gap it is restoring;
        // anything deeper than that is a penetration, and the next move's
        // depenetration is what owns it.
        let fall = (hit.t * probe - self.config.skin_width).clamp(-self.config.skin_width, probe);
        self.position -= UP * fall;
        self.ground = Some(GroundContact {
            normal: hit.normal,
            point: hit.point,
            collider,
        });
    }

    // ── Sweeping ───────────────────────────────────────────────────────

    /// How far the capsule gets along `delta` before something stops it, less
    /// a skin width, and never past `delta` itself.
    fn clear_travel(&self, world: &mut PhysicsWorld, from: DVec3, delta: DVec3) -> f64 {
        let distance = delta.length();
        if distance <= MIN_MOVE {
            return 0.0;
        }
        match self.sweep(world, from, from + delta) {
            None => distance,
            Some((_, hit)) => (hit.t * distance - self.config.skin_width).clamp(0.0, distance),
        }
    }

    /// The capsule's own sweep, with the character's body left out of it.
    fn sweep(
        &self,
        world: &mut PhysicsWorld,
        from: DVec3,
        to: DVec3,
    ) -> Option<(ColliderId, ShapeHit)> {
        world.sweep_capsule_excluding(
            &Segment::new(from, to),
            self.config.radius,
            self.config.half_height,
            self.self_collider,
        )
    }
}

/// Redirect `motion` so it runs along the planes it has collected, after
/// `SV_FlyMove`'s plane-set rule.
///
/// The first plane whose clipped result does not drive into any of the others
/// wins. When none does, two planes have a crease to run along — their cross
/// product — and anything more is a corner with nowhere to go.
///
/// `current` is the displacement as the last clip left it, which is what the
/// crease is measured against; Quake takes it from the same place.
fn clip_to_planes(motion: DVec3, planes: &[DVec3], current: DVec3) -> DVec3 {
    for (i, &plane) in planes.iter().enumerate() {
        let clipped = motion - plane * motion.dot(plane);
        if planes
            .iter()
            .enumerate()
            .all(|(j, &other)| j == i || clipped.dot(other) >= 0.0)
        {
            return clipped;
        }
    }
    if let [first, second] = planes {
        // Normalised, where Quake leaves the cross product at the sine of the
        // angle between the planes and quietly shortens the move by it. Quake
        // III normalises for the same reason.
        let crease = first.cross(*second).normalize_or_zero();
        return crease * crease.dot(current);
    }
    DVec3::ZERO
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collider::{BoxCollider, Sphere};

    /// A ramp big enough that its curvature does not move the slope under the
    /// character: over the tenth of a metre these tests walk, the contact angle
    /// changes by the travel over this radius.
    const DOME_RADIUS: f64 = 1000.0;

    /// The centre a capsule needs so its feet are at `feet`.
    fn centre_for_feet(config: &CharacterConfig, feet: f64) -> f64 {
        feet + config.radius + config.half_height
    }

    /// A floor whose top is `y = 0`, wide enough that nothing in these tests
    /// reaches its edge.
    fn flat_world() -> PhysicsWorld {
        let mut world = PhysicsWorld::new();
        world.add_box(BoxCollider::new(
            DVec3::new(0.0, -1.0, 0.0),
            DVec3::new(50.0, 1.0, 50.0),
        ));
        world
    }

    /// A character standing on `flat_world`'s floor at the origin, already
    /// grounded — the zero move is what finds the floor.
    fn standing(world: &mut PhysicsWorld, config: CharacterConfig) -> CharacterController {
        let mut character =
            CharacterController::new(config, DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0));
        character.move_and_slide(world, DVec3::ZERO);
        assert!(character.is_grounded(), "the fixture starts on the floor");
        character
    }

    /// A dome whose summit is `y = 0`, so a character on it stands on a slope
    /// of whatever angle `on_dome` places it at.
    fn dome_world() -> PhysicsWorld {
        let mut world = PhysicsWorld::new();
        world.add_sphere(Sphere::new(DVec3::new(0.0, -DOME_RADIUS, 0.0), DOME_RADIUS));
        world
    }

    /// The capsule centre that touches the dome exactly where its surface
    /// normal is `angle` from vertical, leaning toward `+X`.
    ///
    /// Exact rather than approximate: a capsule against a sphere is its centre
    /// against that sphere grown along Y (see `query`'s swept-capsule note), so
    /// the contact normal is the radial direction from the grown shape's top
    /// cap, which is what this offsets from.
    fn on_dome(config: &CharacterConfig, angle: f64) -> DVec3 {
        let normal = DVec3::new(angle.sin(), angle.cos(), 0.0);
        DVec3::new(0.0, -DOME_RADIUS + config.half_height, 0.0)
            + normal * (DOME_RADIUS + config.radius)
    }

    /// A floor over `x < 0` whose top is `y = 0`, and a second one over
    /// `x > 0` whose top is `y = height`.
    fn stepped_world(height: f64) -> PhysicsWorld {
        let mut world = PhysicsWorld::new();
        world.add_box(BoxCollider::new(
            DVec3::new(-25.0, -1.0, 0.0),
            DVec3::new(25.0, 1.0, 5.0),
        ));
        world.add_box(BoxCollider::new(
            DVec3::new(25.0, (height - 2.0) * 0.5, 0.0),
            DVec3::new(25.0, (height + 2.0) * 0.5, 5.0),
        ));
        world
    }

    fn feet_of(character: &CharacterController) -> f64 {
        character.position().y - character.config().radius - character.config().half_height
    }

    // ── The unobstructed case ──────────────────────────────────────────

    #[test]
    fn an_unobstructed_move_covers_the_whole_displacement_and_slides_none() {
        let mut world = flat_world();
        let mut character = standing(&mut world, CharacterConfig::default());
        let outcome = character.move_and_slide(&mut world, DVec3::new(0.2, 0.0, -0.1));

        assert_eq!(outcome.slides, 0, "nothing was in the way");
        assert!(
            (outcome.motion - DVec3::new(0.2, 0.0, -0.1)).length() < 1e-12,
            "asked for (0.2, 0, -0.1) and got {:?}",
            outcome.motion,
        );
        assert!(outcome.grounded);
        assert!(!outcome.hit_wall && !outcome.hit_ceiling && !outcome.stepped_up);
    }

    // ── Walls ──────────────────────────────────────────────────────────

    /// **Walking into a wall at an angle keeps the speed along it.** Stopping
    /// dead against a wall the character is only grazing is the failure this
    /// guards; the tangential half of the displacement has to survive intact,
    /// not merely mostly.
    #[test]
    fn walking_into_a_wall_at_an_angle_keeps_the_whole_speed_along_it() {
        let mut world = flat_world();
        // Its -X face is x = 0, so a capsule centred at x = -0.35 has 0.05 of
        // clear approach in front of its flank.
        world.add_box(BoxCollider::new(
            DVec3::new(4.5, 2.0, 0.0),
            DVec3::new(4.5, 3.0, 10.0),
        ));
        let config = CharacterConfig::default();
        let mut character = standing(&mut world, config);
        character.set_position(DVec3::new(-0.35, centre_for_feet(&config, 0.0), 0.0));
        character.move_and_slide(&mut world, DVec3::ZERO);

        let asked = DVec3::new(0.1, 0.0, 0.1);
        let outcome = character.move_and_slide(&mut world, asked);

        assert!(outcome.hit_wall, "the wall's -X face is 0.05 away");
        assert!(
            outcome.motion.x < asked.x,
            "the wall must have taken something off the approach, and x moved {}",
            outcome.motion.x,
        );
        assert!(
            (outcome.motion.z - asked.z).abs() < 1e-12,
            "the speed along the wall is {} and was asked to be {}",
            outcome.motion.z,
            asked.z,
        );
    }

    /// **A move far longer than the obstacle is thick still stops at it.**
    /// The whole point of sweeping rather than stepping-and-testing.
    #[test]
    fn a_move_far_longer_than_the_obstacle_does_not_pass_through_it() {
        let mut world = flat_world();
        let pane_x = 5.0;
        let pane_half_thickness = 0.005;
        world.add_box(BoxCollider::new(
            DVec3::new(pane_x, 2.0, 0.0),
            DVec3::new(pane_half_thickness, 3.0, 10.0),
        ));
        let config = CharacterConfig::default();
        let mut character = standing(&mut world, config);

        let outcome = character.move_and_slide(&mut world, DVec3::new(50.0, 0.0, 0.0));

        assert!(outcome.hit_wall);
        let front = pane_x - pane_half_thickness - config.radius;
        assert!(
            character.position().x <= front,
            "the capsule's flank is at {} and the pane's near face at {front}",
            character.position().x + config.radius,
        );
    }

    // ── Slopes ─────────────────────────────────────────────────────────

    /// **The slope the controller stops walking is the one it was configured
    /// with**, measured rather than assumed.
    ///
    /// The sweep: place a character on a dome at every tenth of a degree from
    /// `0°` to `89.9°`, ground it, then hand it sixty ticks of pure gravity and
    /// no horizontal intent at all, and record how far it drifted sideways. On
    /// ground it can stand on the answer is exactly zero — that is
    /// `ground_adjusted`'s refusal to creep. On ground it cannot, gravity is
    /// clipped to the surface and the character slides down it.
    ///
    /// Measured with [`CharacterConfig::default`]: the drift stays at zero up
    /// to the configured limit and is non-zero one step past it, which brackets
    /// the limit to within the sweep's own step. The comparison is `>=`, so a
    /// slope exactly at the limit is walkable.
    ///
    /// **The bracket is closed at both ends, and it has to be.** Which of the
    /// two adjacent samples the limit lands on is not portable: the dome's
    /// surface normal arrives through `sin`/`cos`, those are platform
    /// transcendentals, and a last-ulp difference moves a slope that sits
    /// exactly on the threshold to either side of it. Linux measures the limit
    /// as the last still sample and Windows as the first creeping one, from the
    /// same source. Asserting an open end pins the measurement finer than the
    /// sweep's step can resolve, which is a claim about a libm rather than
    /// about this controller.
    #[test]
    fn the_slope_the_controller_stops_walking_is_the_one_it_was_configured_with() {
        let config = CharacterConfig::default();
        let step_degrees = 0.1;
        // Big enough to cross the skin gap in one tick: a fall shorter than
        // that never touches the ground and the ground never gets a say.
        let gravity = DVec3::new(0.0, -0.05, 0.0);

        let mut last_still: Option<f64> = None;
        let mut first_creep: Option<f64> = None;
        for tenth in 0..900 {
            let degrees = f64::from(tenth) * step_degrees;
            let mut world = dome_world();
            let mut character =
                CharacterController::new(config, on_dome(&config, degrees.to_radians()));
            character.move_and_slide(&mut world, DVec3::ZERO);
            let start = character.position();
            for _ in 0..60 {
                character.move_and_slide(&mut world, gravity);
            }
            let drift = (character.position() - start).x;
            if first_creep.is_none() {
                if drift == 0.0 {
                    last_still = Some(degrees);
                } else {
                    first_creep = Some(degrees);
                }
            }
        }

        let (Some(last_still), Some(first_creep)) = (last_still, first_creep) else {
            panic!(
                "the sweep never crossed a boundary: {last_still:?} was the last still \
                 slope and {first_creep:?} the first creeping one",
            );
        };
        let limit = config.min_ground_normal_y.acos().to_degrees();
        assert!(
            last_still <= limit && limit <= first_creep,
            "the boundary was measured at {last_still}° still / {first_creep}° creeping, \
             and the configured limit is {limit}° — the two samples are one \
             {step_degrees}° step apart, so the limit has to fall between them",
        );
    }

    /// **A surface exactly at the limit is walkable**, which the sweep above
    /// cannot say.
    ///
    /// That sweep brackets the boundary to a tenth of a degree and no finer,
    /// and which of its two adjacent samples the limit lands on moves with the
    /// platform's `sin`/`cos` — so flipping `is_walkable`'s comparison from
    /// `>=` to `>` shifts it by exactly the amount a different libm does, and
    /// the sweep cannot tell the two apart. This can: the normal is built
    /// **from** [`CharacterConfig::min_ground_normal_y`] rather than from an
    /// angle, so its rise is that number bit for bit on every target, and no
    /// transcendental stands between the configuration and the answer.
    #[test]
    fn a_surface_exactly_at_the_limit_is_walkable() {
        let config = CharacterConfig::default();
        let character = CharacterController::new(config, DVec3::ZERO);
        let rise = config.min_ground_normal_y;
        // Unit length, and its Y component is `rise` exactly — the `sqrt` only
        // ever touches the horizontal lane.
        let exactly = DVec3::new((1.0 - rise * rise).sqrt(), rise, 0.0);

        assert_eq!(
            exactly.y, rise,
            "the test's own normal must carry the configured rise unrounded"
        );
        assert!(
            character.is_walkable(exactly),
            "a surface whose rise is exactly {rise} must be walkable, because the \
             comparison is `>=` and the limit is the last slope you can stand on"
        );
    }

    /// **Walking up a walkable slope covers the same horizontal ground as
    /// walking on the flat**, which is Unreal's
    /// `bMaintainHorizontalGroundVelocity` and the reason a ramp does not feel
    /// like treacle.
    #[test]
    fn a_walkable_slope_is_climbed_without_losing_horizontal_ground() {
        let config = CharacterConfig::default();
        let angle = 30.0_f64.to_radians();
        let mut world = dome_world();
        let mut character = CharacterController::new(config, on_dome(&config, angle));
        character.move_and_slide(&mut world, DVec3::ZERO);
        assert!(character.is_grounded(), "30° is inside the default limit");

        // Uphill is -X: the character is on the +X flank of the dome.
        let asked = DVec3::new(-0.05, 0.0, 0.0);
        let before = character.position();
        let outcome = character.move_and_slide(&mut world, asked);
        let moved = character.position() - before;

        assert!(
            (moved.x - asked.x).abs() < 1e-6,
            "the horizontal ground covered was {} and the request was {}",
            moved.x,
            asked.x,
        );
        assert!(
            (moved.y - asked.x.abs() * angle.tan()).abs() < 1e-4,
            "climbing 0.05 m of a 30° slope should rise {}, and it rose {}",
            asked.x.abs() * angle.tan(),
            moved.y,
        );
        assert!(outcome.grounded);
    }

    // ── Steps ──────────────────────────────────────────────────────────

    /// **The step the controller stops climbing is the offset it was
    /// configured with**, measured rather than assumed.
    ///
    /// The sweep: build a floor and a raised second floor at every millimetre
    /// from `0` to `0.6 m`, walk a grounded character into the join for thirty
    /// ticks of 0.05 m, and record whether it ended up on top.
    ///
    /// Measured with [`CharacterConfig::default`]: every step through `0.409 m`
    /// is climbed and `0.410 m` is the first that is not. The band above the
    /// configured offset is one [`CharacterConfig::skin_width`] wide, because
    /// a settled capsule already floats that far off its floor and the rise
    /// starts from there.
    #[test]
    fn the_step_the_controller_stops_climbing_is_the_offset_it_was_configured_with() {
        let config = CharacterConfig::default();
        let step_metres = 0.001;

        let mut last_climbed: Option<f64> = None;
        let mut first_blocked: Option<f64> = None;
        for milli in 0..=600 {
            let height = f64::from(milli) * step_metres;
            let mut world = stepped_world(height);
            let mut character = CharacterController::new(
                config,
                DVec3::new(-1.0, centre_for_feet(&config, 0.0), 0.0),
            );
            character.move_and_slide(&mut world, DVec3::ZERO);
            for _ in 0..30 {
                character.move_and_slide(&mut world, DVec3::new(0.05, 0.0, 0.0));
            }
            let climbed =
                character.position().x > 0.2 && (feet_of(&character) - height).abs() < 0.02;
            if first_blocked.is_none() {
                if climbed {
                    last_climbed = Some(height);
                } else {
                    first_blocked = Some(height);
                }
            }
        }

        let last_climbed = last_climbed.expect("a zero-height step is walked straight over");
        let first_blocked = first_blocked.expect("a 0.6 m step is not a step");
        assert!(
            last_climbed >= config.step_offset,
            "a step of exactly the configured offset must be climbed, and the last \
             one climbed was {last_climbed}",
        );
        assert!(
            first_blocked <= config.step_offset + config.skin_width + step_metres,
            "the first step refused was {first_blocked}, which is more than one skin \
             width above the configured offset of {}",
            config.step_offset,
        );
    }

    /// **Walking off a lip inside the step offset keeps the character on the
    /// ground**, rather than dropping it into a one-tick fall on every stair.
    #[test]
    fn walking_off_a_lip_within_the_step_offset_never_leaves_the_ground() {
        let config = CharacterConfig::default();
        let drop = 0.3;
        let mut world = stepped_world(-drop);
        let mut character =
            CharacterController::new(config, DVec3::new(-1.0, centre_for_feet(&config, 0.0), 0.0));
        character.move_and_slide(&mut world, DVec3::ZERO);

        for tick in 0..30 {
            let outcome = character.move_and_slide(&mut world, DVec3::new(0.05, 0.0, 0.0));
            assert!(
                outcome.grounded,
                "tick {tick} left the ground at x = {}, y = {}",
                character.position().x,
                character.position().y,
            );
        }
        assert!(character.position().x > 0.2, "it walked past the lip");
        assert!(
            (feet_of(&character) + drop).abs() < config.skin_width * 2.0,
            "it should be standing on the lower floor at {}, and its feet are at {}",
            -drop,
            feet_of(&character),
        );
    }

    /// The other half of the snap: a drop deeper than the step offset is a
    /// fall, and the character has to leave the ground for it.
    #[test]
    fn walking_off_a_drop_past_the_step_offset_leaves_the_ground() {
        let config = CharacterConfig::default();
        let mut world = stepped_world(-(config.step_offset * 2.0));
        let mut character =
            CharacterController::new(config, DVec3::new(-1.0, centre_for_feet(&config, 0.0), 0.0));
        character.move_and_slide(&mut world, DVec3::ZERO);

        let mut left_the_ground = false;
        for _ in 0..30 {
            left_the_ground |= !character
                .move_and_slide(&mut world, DVec3::new(0.05, 0.0, 0.0))
                .grounded;
        }
        assert!(
            left_the_ground,
            "the character walked over a {} m drop without ever falling",
            config.step_offset * 2.0,
        );
    }

    /// **A character pressed into a corner does not climb it.** The step-up's
    /// advance is what refuses: after rising, a corner is still a wall in both
    /// directions, so there is nowhere to step onto and the whole step is
    /// reverted rather than leaving the capsule up in the air.
    #[test]
    fn a_character_pressed_into_a_corner_does_not_climb_it() {
        let config = CharacterConfig::default();
        let mut world = flat_world();
        world.add_box(BoxCollider::new(
            DVec3::new(5.5, 2.0, 0.0),
            DVec3::new(5.0, 3.0, 10.0),
        ));
        world.add_box(BoxCollider::new(
            DVec3::new(0.0, 2.0, 5.5),
            DVec3::new(10.0, 3.0, 5.0),
        ));
        let mut character = standing(&mut world, config);
        let resting_feet = feet_of(&character);

        let mut planes_at_once = 0;
        for _ in 0..60 {
            let outcome = character.move_and_slide(&mut world, DVec3::new(0.05, 0.0, 0.05));
            planes_at_once = planes_at_once.max(outcome.slides);
            assert!(
                feet_of(&character) <= resting_feet + config.skin_width,
                "the character rose to {} from {resting_feet}",
                feet_of(&character),
            );
            assert!(
                !outcome.stepped_up,
                "a corner is not a step, and it was climbed at y = {}",
                character.position().y,
            );
        }
        assert!(
            character.position().x > 0.1 && character.position().z > 0.1,
            "the character should have slid along both walls before wedging, and \
             stopped at {:?}",
            character.position(),
        );
        assert!(
            planes_at_once >= 2,
            "the corner never presented two planes in one move, so the clipping \
             this is about never ran",
        );
    }

    // ── Depenetration ──────────────────────────────────────────────────

    /// **A capsule that wakes up inside a box leaves through the nearest
    /// face**, by the depth it is in by, and not by a teleport to somewhere
    /// convenient.
    #[test]
    fn a_capsule_that_starts_inside_a_box_is_pushed_out_of_the_nearest_face() {
        let config = CharacterConfig::default();
        let mut world = PhysicsWorld::new();
        let half = DVec3::new(1.0, 1.0, 1.0);
        world.add_box(BoxCollider::new(DVec3::ZERO, half));

        // Buried 0.2 m short of the +X face and dead centre on the others, so
        // +X is the only shortest way out.
        let inside = DVec3::new(half.x - 0.2, 0.0, 0.0);
        let mut character = CharacterController::new(config, inside);
        let outcome = character.move_and_slide(&mut world, DVec3::ZERO);

        let mut still_inside = Vec::new();
        world.capsule_penetrations_into(&character.capsule(), None, &mut still_inside);
        assert!(
            still_inside.is_empty(),
            "the capsule is still inside {still_inside:?} after being pushed out",
        );
        let expected = DVec3::X * (config.radius + 0.2 + config.skin_width);
        assert!(
            (outcome.depenetration - expected).length() < 1e-12,
            "expected a push of {expected:?} and got {:?}",
            outcome.depenetration,
        );
    }

    /// The push has to converge: a character standing clear of everything must
    /// not be shoved on every tick for ever.
    #[test]
    fn a_capsule_clear_of_the_world_is_not_pushed_at_all() {
        let config = CharacterConfig::default();
        let mut world = flat_world();
        // Set down at the gap a settled capsule keeps, so it is touching
        // nothing — and the move must leave it exactly where it is.
        let mut character = CharacterController::new(
            config,
            DVec3::new(0.0, centre_for_feet(&config, config.skin_width), 0.0),
        );

        let outcome = character.move_and_slide(&mut world, DVec3::ZERO);

        assert_eq!(
            outcome.depenetration,
            DVec3::ZERO,
            "a capsule a skin width clear of its floor is inside nothing",
        );
        assert!(
            outcome.grounded,
            "and it is standing on the floor it is clear of"
        );
    }

    // ── Jumping and the ground snap ────────────────────────────────────

    /// An upward move is used exactly as it was asked for: the ground
    /// adjustment stands aside the moment the character asks to leave the
    /// ground, or a jump would be flattened into a walk.
    #[test]
    fn an_upward_move_covers_its_whole_rise() {
        let mut world = flat_world();
        let mut character = standing(&mut world, CharacterConfig::default());
        let before = character.position().y;

        character.move_and_slide(&mut world, DVec3::new(0.0, 0.2, 0.0));

        assert!(
            (character.position().y - before - 0.2).abs() < 1e-12,
            "asked to rise 0.2 and rose {}",
            character.position().y - before,
        );
    }

    /// And the ground snap does not reach up after it. The snap is for a
    /// character that was walking and stayed down, not for one that jumped.
    #[test]
    fn an_upward_move_is_not_pulled_back_by_the_ground_snap() {
        let mut world = flat_world();
        let mut character = standing(&mut world, CharacterConfig::default());

        let outcome = character.move_and_slide(&mut world, DVec3::new(0.0, 0.2, 0.0));

        assert!(
            !outcome.grounded,
            "the floor reached up and took the jump back"
        );
    }

    /// **A grounded character asked for nothing but gravity does not move.**
    /// On the flat that is trivial; on a slope it is the whole of
    /// `floor_stop_on_slope`, because gravity clipped against a 40° face has a
    /// downhill component on every single tick.
    #[test]
    fn gravity_alone_does_not_walk_a_grounded_character_down_a_slope() {
        let config = CharacterConfig::default();
        let mut world = dome_world();
        let mut character =
            CharacterController::new(config, on_dome(&config, 40.0_f64.to_radians()));
        character.move_and_slide(&mut world, DVec3::ZERO);
        assert!(character.is_grounded(), "40° is inside the default limit");

        let before = character.position();
        for tick in 0..60 {
            let outcome = character.move_and_slide(&mut world, DVec3::new(0.0, -0.05, 0.0));
            assert_eq!(
                (outcome.motion.x, outcome.motion.z),
                (0.0, 0.0),
                "tick {tick} walked sideways",
            );
            assert!(outcome.grounded, "tick {tick} left the slope");
        }
        let drift = character.position() - before;
        assert_eq!(
            (drift.x, drift.z),
            (0.0, 0.0),
            "it ended up downhill by {drift:?}"
        );
        assert!(
            drift.y.abs() < config.skin_width,
            "it sank {} into the slope, which is more than the gap it settles to",
            -drift.y,
        );
    }

    // ── The character's own collider ───────────────────────────────────

    /// A character registered in the world would otherwise find itself at
    /// `t = 0` on every sweep and never move at all; naming its own collider
    /// is what keeps it out of its own answers, and each move writes the new
    /// position back so the world's copy is never a tick behind.
    #[test]
    fn a_character_registered_in_the_world_is_left_out_of_its_own_sweeps() {
        let config = CharacterConfig::default();
        let mut world = flat_world();
        let body = world.add_capsule(Capsule::new(
            DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0),
            config.radius,
            config.half_height,
        ));
        let mut character =
            CharacterController::new(config, DVec3::new(0.0, centre_for_feet(&config, 0.0), 0.0))
                .with_self_collider(body);

        character.move_and_slide(&mut world, DVec3::ZERO);

        let outcome = character.move_and_slide(&mut world, DVec3::new(0.2, 0.0, 0.0));
        assert!(
            (outcome.motion.x - 0.2).abs() < 1e-12,
            "its own body blocked it: it moved {}",
            outcome.motion.x,
        );
        let aabb = world.aabb_of(body).expect("the body is still registered");
        assert!(
            (aabb.centre() - character.position()).length() < 1e-12,
            "the world's copy is at {:?} and the character at {:?}",
            aabb.centre(),
            character.position(),
        );
    }
}
