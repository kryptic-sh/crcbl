//! Soft Step: substepped sequential impulses with soft contacts, warm
//! starting, speculative points and a restitution pass —
//! `docs/plan/36-contact-solver.md` decision 1, after Box2D v3's
//! `contact_solver.c`.
//!
//! ```text
//!   prepare     anchors, effective masses, softness, approach speeds,
//!               last tick's impulses by feature id
//!   ×substeps   integrate velocities   (forces, gyroscopic step, speed caps)
//!               warm start             (apply the impulses carried so far)
//!               solve                  (soft: a spring-damper pushes out)
//!               integrate positions
//!               relax                  (rigid: remove the push's velocity)
//!   restitution one pass over points that approached above the threshold
//!   store       impulses back into the contacts, KineticContact events
//! ```
//!
//! # Why it is shaped like this
//!
//! **Soft contacts** replace Baumgarte's bias: the push-out is a spring at
//! [`super::ContactSettings::contact_hertz`] with damping, capped at
//! [`super::ContactSettings::push_out_speed`], so a deep overlap is resolved over a
//! few substeps rather than in one violent correction that adds energy. The
//! **relax** pass after the positions move takes back the velocity the spring
//! added, so a body pushed out does not keep flying. **Separation is tracked
//! within the tick** from each body's motion since the manifold was built, so
//! collision runs once a tick and every substep still sees how far apart the
//! points are now.
//!
//! That motion's turning part is taken **to first order** — the anchor moved
//! by the accumulated rotation vector crossed with it — and not by turning the
//! anchor through the body's rotation, which is Box2D's form. The two agree for
//! a corner, and disagree for a rolling ball: a ball's contact is always at the
//! bottom of it, but an anchor turned with the ball climbs away from the
//! surface, so a ball rolling at 35 rad/s seemed a centimetre sunk half a tick
//! later and was pushed up off the slope it was rolling down — measured, it
//! slipped at 14 cm/s where it should have rolled. To first order the
//! turning is perpendicular to the anchor, and a ball's rolling moves its
//! contact nowhere along the normal.
//!
//! A **speculative point** — positive separation `s` — lets the bodies close
//! at most `s` in one substep, which is what stops a fast body at the surface
//! instead of inside it.
//!
//! **Restitution** runs once, after every substep, on the approach speed each
//! point had before the tick; Rapier moved it out of the substeps because
//! speculative contacts damped bounces there.
//!
//! **Friction acts at the manifold's centroid**, as decision 2 has it after
//! Box3D and Rapier: one two-axis tangential impulse at the mean of the
//! points, bounded by `μ` times their summed normal impulse, and one twist
//! impulse about the normal, bounded by `μ Σ λᵢ rᵢ` — each point's share of
//! the friction at its distance `rᵢ` from the centroid, the most torque the
//! points' own friction could resist. Friction at the centroid alone would
//! let a box spin freely on its face, since its centroid does not move; the
//! twist is what stops it. A one-point manifold's tangential friction is
//! exactly the point friction rung 1 had.
//!
//! **A one-point manifold twists against its contact patch.** Its one point is
//! at the centroid, so `Σ λᵢ rᵢ` would be zero, and a ball spinning about its
//! contact normal would spin for ever — measured on 2026-09-23, one did on a
//! bin floor of the obstacle wall at 1.22 rad/s, and never slept. A real
//! contact is a patch, not a point, and uniform pressure over a disc of radius
//! `a` resists twist with a torque of up to `⅔ μ N a`; here that bound is
//! `μ λ a` with `a` the patch radius [`patch_radius`] gives — Hertz's `√(R δ)`
//! for the pair's curvature `R` and the point's depth `δ`, no smaller than
//! [`MIN_PATCH_RADIUS`]. Taking `a` for `⅔ a` is a rounding well inside how
//! little the soft contact's depth says about a real patch. A manifold of two
//! points or more twists exactly as before.
//!
//! # The interior is scalar `f64`
//!
//! Decision 7 makes the interior `f32` and decision 8 makes it wide, at
//! rung 6. Every per-point operation here reads only its own constraint and its
//! two bodies, in one order, so a wide kernel can replace [`solve_pass`] and its
//! siblings lane for lane.

use core::f64::consts::TAU;

use crcbl_core::Pool;
use glam::{DMat3, DVec3};

use super::island::Islands;
use super::manifold::{LINEAR_SLOP, MAX_POINTS, orthonormal_basis};
use super::shape::ContactShape;
use super::{Bodies, ContactPipeline, KineticContact, KineticSource, WarmImpulses};
use crate::components::{RigidBody, Transform};
use crate::integrator::{SemiImplicitEuler, SpinStep};
use crate::system::{AwakeSet, BodyRecord, StaticSet};

/// The index a constraint uses for a side that does not step.
const NONE: usize = usize::MAX;

/// The smallest contact patch a one-point manifold twists against, in
/// metres: a tenth of Box2D's linear slop, half a millimetre.
///
/// The soft contact rests almost at zero depth, where Hertz's `√(R δ)` would
/// give no patch and the spin would never stop. Half a millimetre is about
/// the patch real Hertz contact gives the props this solver is tuned for: a
/// 0.2 kg ball of 7 cm radius in hard plastic resting on a floor of the same
/// (`E ≈ 2 GPa` each, so `E* ≈ 1.1 GPa`) has `a = (3 F R / 4 E*)^⅓ ≈ 0.45 mm`.
/// So the floor never claims much more grip than such a patch has; it only
/// makes a spin stop in finite time.
const MIN_PATCH_RADIUS: f64 = 0.1 * LINEAR_SLOP;

/// The radius of the patch a one-point contact between `a` and `b` presses
/// over, `separation` deep: Hertz's `√(R δ)`, with `R` the pair's effective
/// radius of curvature `1 / (1/R_a + 1/R_b)`, clamped between
/// [`MIN_PATCH_RADIUS`] and `R` itself.
///
/// A sphere and a capsule curve at their radius; a box face, a mesh's
/// triangle and a plane are flat and add no curvature. Two flat shapes meeting at one point meet at a
/// corner or an edge, whose patch is the smallest there is.
fn patch_radius(a: &ContactShape, b: &ContactShape, separation: f64) -> f64 {
    let curvature = |shape: &ContactShape| match *shape {
        ContactShape::Sphere { radius, .. } | ContactShape::Capsule { radius, .. } => 1.0 / radius,
        ContactShape::Box { .. } | ContactShape::Triangle { .. } | ContactShape::Plane { .. } => {
            0.0
        }
    };
    let total = curvature(a) + curvature(b);
    if total <= 0.0 {
        return MIN_PATCH_RADIUS;
    }
    let radius = 1.0 / total;
    (radius * separation.min(0.0).abs())
        .sqrt()
        .clamp(MIN_PATCH_RADIUS.min(radius), radius)
}

/// The buffers a solve works in, kept between ticks.
#[derive(Debug, Default)]
pub(crate) struct Scratch {
    bodies: Vec<SolverBody>,
    constraints: Vec<Constraint>,
    /// The points the restitution pass bounced, as (constraint, point).
    bounced: Vec<(usize, usize)>,
}

/// What the solver keeps for one awake body over a tick.
#[derive(Clone, Copy, Debug)]
struct SolverBody {
    inverse_mass: f64,
    /// The inverse inertia in the world, at the tick's start orientation.
    inverse_inertia: DMat3,
    start: DVec3,
    /// How far it has moved since the manifolds were built.
    delta_position: DVec3,
    /// How far it has turned since, as the sum of each substep's angular
    /// velocity times the substep: a rotation vector to first order.
    delta_angle: DVec3,
    spin: SpinStep,
}

impl SolverBody {
    fn new(body: &RigidBody, transform: &Transform) -> Self {
        let inverse_inertia = if body.has_rotational_inertia() {
            let turn = DMat3::from_quat(transform.rotation);
            turn * body.inverse_local_inertia * turn.transpose()
        } else {
            DMat3::ZERO
        };
        Self {
            inverse_mass: body.inverse_mass,
            inverse_inertia,
            start: transform.position,
            delta_position: DVec3::ZERO,
            delta_angle: DVec3::ZERO,
            spin: SpinStep::default(),
        }
    }
}

/// A soft constraint's coefficients for one substep length — Box2D's
/// `b2MakeSoft`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Softness {
    bias_rate: f64,
    mass_scale: f64,
    impulse_scale: f64,
}

impl Softness {
    /// A spring of `hertz` with damping ratio `zeta`, stepped by `h`.
    fn new(hertz: f64, zeta: f64, h: f64) -> Self {
        if hertz == 0.0 {
            return Self {
                bias_rate: 0.0,
                mass_scale: 1.0,
                impulse_scale: 0.0,
            };
        }
        let omega = TAU * hertz;
        let a1 = 2.0 * zeta + h * omega;
        let a2 = h * omega * a1;
        let a3 = 1.0 / (1.0 + a2);
        Self {
            bias_rate: omega / a1,
            mass_scale: a2 * a3,
            impulse_scale: a3,
        }
    }
}

/// One point of a constraint.
#[derive(Clone, Copy, Debug, Default)]
struct Point {
    /// World position when the manifold was built.
    position: DVec3,
    /// From each body's centre, at the start of the tick.
    anchor_a: DVec3,
    anchor_b: DVec3,
    /// The separation when the manifold was built: the current separation is
    /// this plus the anchors' relative motion along the normal since.
    separation: f64,
    normal_mass: f64,
    normal_impulse: f64,
    /// How far it lies from the manifold's centroid: the lever its friction
    /// has against a twist — or, for a manifold of one point, the radius of
    /// its contact patch.
    twist_radius: f64,
    max_normal_impulse: f64,
    /// Every normal impulse the tick delivered.
    total_normal_impulse: f64,
    /// The normal speed before the tick: negative approaching.
    relative_velocity: f64,
}

/// One contact, prepared for a tick.
#[derive(Clone, Copy, Debug)]
struct Constraint {
    slot: usize,
    a: usize,
    b: usize,
    normal: DVec3,
    tangents: [DVec3; 2],
    friction: f64,
    restitution: f64,
    softness: Softness,
    count: usize,
    points: [Point; MAX_POINTS],
    /// The manifold's friction: one tangential impulse at the centroid of its
    /// points, from each body's centre.
    friction_anchor_a: DVec3,
    friction_anchor_b: DVec3,
    tangent_mass: [f64; 2],
    tangent_impulse: [f64; 2],
    /// And the twist: an angular impulse about the normal.
    twist_mass: f64,
    twist_impulse: f64,
    /// For the event: who, how heavy and how fast at the start.
    entity_a: Option<crcbl_ecs::Entity>,
    entity_b: Option<crcbl_ecs::Entity>,
    mass_a: f64,
    mass_b: f64,
    velocity_a: DVec3,
    velocity_b: DVec3,
    /// The first point's relative velocity, `B` less `A`, at the start.
    point_velocity: DVec3,
}

/// A body's linear and angular velocity, or zero for a side that does not
/// step.
fn velocity(bodies: &[RigidBody], index: usize) -> (DVec3, DVec3) {
    if index == NONE {
        (DVec3::ZERO, DVec3::ZERO)
    } else {
        (bodies[index].velocity, bodies[index].angular_velocity)
    }
}

/// Apply `impulse` at `anchor` to body `index`, which is a no-op for a side
/// that does not step and for one of infinite mass.
fn apply(
    bodies: &mut [RigidBody],
    solver: &[SolverBody],
    index: usize,
    anchor: DVec3,
    impulse: DVec3,
) {
    if index == NONE {
        return;
    }
    let s = &solver[index];
    let body = &mut bodies[index];
    body.velocity += impulse * s.inverse_mass;
    body.angular_velocity += s.inverse_inertia * anchor.cross(impulse);
}

/// Apply the angular impulse `impulse` to body `index`, a no-op for a side
/// that does not step.
fn apply_angular(bodies: &mut [RigidBody], solver: &[SolverBody], index: usize, impulse: DVec3) {
    if index == NONE {
        return;
    }
    bodies[index].angular_velocity += solver[index].inverse_inertia * impulse;
}

/// The velocity of `B`'s point less `A`'s.
fn relative_velocity(bodies: &[RigidBody], c: &Constraint, p: &Point) -> DVec3 {
    relative_velocity_at(bodies, c, p.anchor_a, p.anchor_b)
}

/// The velocity of `B` at `anchor_b` less `A`'s at `anchor_a`.
fn relative_velocity_at(
    bodies: &[RigidBody],
    c: &Constraint,
    anchor_a: DVec3,
    anchor_b: DVec3,
) -> DVec3 {
    let (va, wa) = velocity(bodies, c.a);
    let (vb, wb) = velocity(bodies, c.b);
    vb + wb.cross(anchor_b) - va - wa.cross(anchor_a)
}

/// One body's share of the twist's inverse effective mass: `n · I⁻¹ n`.
fn inverse_twist_mass(solver: &[SolverBody], index: usize, normal: DVec3) -> f64 {
    if index == NONE {
        return 0.0;
    }
    normal.dot(solver[index].inverse_inertia * normal)
}

/// One body's share of a direction's effective mass: `m⁻¹ + d · (I⁻¹(r × d) × r)`.
fn inverse_mass_along(solver: &[SolverBody], index: usize, anchor: DVec3, direction: DVec3) -> f64 {
    if index == NONE {
        return 0.0;
    }
    let s = &solver[index];
    let turned = s.inverse_inertia * anchor.cross(direction);
    s.inverse_mass + direction.dot(turned.cross(anchor))
}

impl ContactPipeline {
    /// One tick of `dt`: every substep, restitution, and the impulses stored.
    /// The bodies' accumulated forces are held for the whole tick and cleared
    /// at its end.
    pub(crate) fn solve(
        &mut self,
        records: &Pool<BodyRecord>,
        statics: &StaticSet,
        awake: &mut AwakeSet,
        islands: &Islands,
        dt: f64,
    ) {
        let settings = self.settings;
        let substeps = settings.substeps.max(1);
        let h = dt / f64::from(substeps);
        self.counters.bounces = 0;
        self.counters.bounce_ratio_sum = 0.0;
        self.counters.restitution_sum = 0.0;
        self.kinetic.clear();

        let mut scratch = std::mem::take(&mut self.solver);
        scratch.bodies.clear();
        scratch.bodies.extend(
            awake
                .bodies
                .iter()
                .zip(&awake.transforms)
                .map(|(body, transform)| SolverBody::new(body, transform)),
        );
        self.prepare(
            Bodies {
                records,
                statics,
                awake,
                islands,
            },
            &mut scratch,
            dt,
            h,
        );

        let AwakeSet {
            transforms, bodies, ..
        } = awake;
        let max_angular_speed = settings.max_rotation / h;
        for _ in 0..substeps {
            for (index, body) in bodies.iter_mut().enumerate() {
                let spin =
                    SemiImplicitEuler::integrate_velocity(body, transforms[index].rotation, h);
                let speed_squared = body.velocity.length_squared();
                if speed_squared > settings.max_linear_speed * settings.max_linear_speed {
                    body.velocity *= settings.max_linear_speed / speed_squared.sqrt();
                }
                let turn_squared = body.angular_velocity.length_squared();
                if turn_squared > max_angular_speed * max_angular_speed {
                    body.angular_velocity *= max_angular_speed / turn_squared.sqrt();
                }
                scratch.bodies[index].spin = spin;
            }

            warm_start(&scratch.constraints, &scratch.bodies, bodies);
            solve_pass(
                &mut scratch.constraints,
                &scratch.bodies,
                bodies,
                h,
                Some(settings.push_out_speed),
            );

            for (index, body) in bodies.iter_mut().enumerate() {
                let transform = &mut transforms[index];
                let s = &mut scratch.bodies[index];
                SemiImplicitEuler::integrate_position(body, transform, s.spin, h);
                s.delta_position = transform.position - s.start;
                s.delta_angle += body.angular_velocity * h;
            }

            solve_pass(&mut scratch.constraints, &scratch.bodies, bodies, h, None);
            for constraint in &mut scratch.constraints {
                for point in &mut constraint.points[..constraint.count] {
                    point.total_normal_impulse += point.normal_impulse;
                }
            }
        }

        self.restitution(&mut scratch, bodies);
        self.store(&scratch, bodies);
        for body in bodies.iter_mut() {
            body.clear_forces();
        }
        self.solver = scratch;
    }

    /// Builds the constraints for every touching contact with an awake
    /// dynamic side. A sleeping island's contacts are passed over on a look
    /// at their records, before either side is placed.
    fn prepare(&self, world: Bodies<'_>, scratch: &mut Scratch, dt: f64, h: f64) {
        let settings = &self.settings;
        let hertz = settings
            .contact_hertz
            .min(0.25 * f64::from(settings.substeps.max(1)) / dt);
        let soft = Softness::new(hertz, settings.damping_ratio, h);
        let stiff = Softness::new(2.0 * hertz, settings.damping_ratio, h);

        scratch.constraints.clear();
        for (slot, contact) in self.contacts.iter().enumerate() {
            let Some(contact) = contact else {
                continue;
            };
            if !contact.touching || !self.solvable(contact.a, contact.b, world) {
                continue;
            }
            let (Some(a), Some(b)) = (self.side(contact.a, world), self.side(contact.b, world))
            else {
                continue;
            };
            let material = a.material.combine(&b.material);
            let normal = contact.manifold.normal;
            let tangents = {
                let (t0, t1) = orthonormal_basis(normal);
                [t0, t1]
            };
            let mut constraint = Constraint {
                slot,
                a: a.awake.unwrap_or(NONE),
                b: b.awake.unwrap_or(NONE),
                normal,
                tangents,
                friction: material.friction,
                restitution: material.restitution,
                softness: if a.inverse_mass == 0.0 || b.inverse_mass == 0.0 {
                    stiff
                } else {
                    soft
                },
                count: contact.manifold.points().len(),
                points: [Point::default(); MAX_POINTS],
                friction_anchor_a: DVec3::ZERO,
                friction_anchor_b: DVec3::ZERO,
                tangent_mass: [0.0; 2],
                tangent_impulse: [0.0; 2],
                twist_mass: 0.0,
                twist_impulse: 0.0,
                entity_a: a.entity,
                entity_b: b.entity,
                mass_a: a.mass,
                mass_b: b.mass,
                velocity_a: a.velocity,
                velocity_b: b.velocity,
                point_velocity: DVec3::ZERO,
            };

            let warm = if settings.warm_starting {
                contact.impulses
            } else {
                WarmImpulses::default()
            };
            let effective = |anchor_a: DVec3, anchor_b: DVec3, direction: DVec3| {
                let k = inverse_mass_along(&scratch.bodies, constraint.a, anchor_a, direction)
                    + inverse_mass_along(&scratch.bodies, constraint.b, anchor_b, direction);
                if k > 0.0 { 1.0 / k } else { 0.0 }
            };
            let points = contact.manifold.points();
            let centroid = points.iter().map(|p| p.point).sum::<DVec3>() / points.len() as f64;
            for (k, mp) in points.iter().enumerate() {
                let anchor_a = mp.point - a.position;
                let anchor_b = mp.point - b.position;
                let dv = b.velocity + b.angular_velocity.cross(anchor_b)
                    - a.velocity
                    - a.angular_velocity.cross(anchor_a);
                if k == 0 {
                    constraint.point_velocity = dv;
                }
                constraint.points[k] = Point {
                    position: mp.point,
                    anchor_a,
                    anchor_b,
                    separation: mp.separation,
                    normal_mass: effective(anchor_a, anchor_b, normal),
                    normal_impulse: warm.normal[k],
                    twist_radius: if points.len() == 1 {
                        patch_radius(&a.shape, &b.shape, mp.separation)
                    } else {
                        (mp.point - centroid).length()
                    },
                    max_normal_impulse: 0.0,
                    total_normal_impulse: 0.0,
                    relative_velocity: dv.dot(normal),
                };
            }
            let (anchor_a, anchor_b) = (centroid - a.position, centroid - b.position);
            constraint.friction_anchor_a = anchor_a;
            constraint.friction_anchor_b = anchor_b;
            constraint.tangent_mass = [
                effective(anchor_a, anchor_b, tangents[0]),
                effective(anchor_a, anchor_b, tangents[1]),
            ];
            constraint.tangent_impulse = [
                warm.friction.dot(tangents[0]),
                warm.friction.dot(tangents[1]),
            ];
            let twist = inverse_twist_mass(&scratch.bodies, constraint.a, normal)
                + inverse_twist_mass(&scratch.bodies, constraint.b, normal);
            constraint.twist_mass = if twist > 0.0 { 1.0 / twist } else { 0.0 };
            constraint.twist_impulse = warm.twist;
            scratch.constraints.push(constraint);
        }
    }

    /// The restitution pass: each point that approached faster than the
    /// threshold, and was pushed at all, is given the separating speed its
    /// restitution asks for.
    ///
    /// The bounce the counters report is read once the whole pass is over, so
    /// it is what the tick left rather than what one point was set to: exactly
    /// the restitution for a lone bounce, and less where a neighbouring
    /// contact's bounce took some back.
    fn restitution(&mut self, scratch: &mut Scratch, bodies: &mut [RigidBody]) {
        let threshold = self.settings.restitution_threshold;
        scratch.bounced.clear();
        for (index, c) in scratch.constraints.iter_mut().enumerate() {
            if c.restitution == 0.0 {
                continue;
            }
            for k in 0..c.count {
                let p = c.points[k];
                if p.relative_velocity > -threshold || p.max_normal_impulse == 0.0 {
                    continue;
                }
                let vn = relative_velocity(bodies, c, &p).dot(c.normal);
                let impulse = -p.normal_mass * (vn + c.restitution * p.relative_velocity);
                let new = (p.normal_impulse + impulse).max(0.0);
                let delta = new - p.normal_impulse;
                let point = &mut c.points[k];
                point.normal_impulse = new;
                point.max_normal_impulse = point.max_normal_impulse.max(delta);
                point.total_normal_impulse += delta;
                apply(bodies, &scratch.bodies, c.a, p.anchor_a, -c.normal * delta);
                apply(bodies, &scratch.bodies, c.b, p.anchor_b, c.normal * delta);
                scratch.bounced.push((index, k));
            }
        }
        for &(index, k) in &scratch.bounced {
            let c = &scratch.constraints[index];
            let p = &c.points[k];
            let after = relative_velocity(bodies, c, p).dot(c.normal);
            self.counters.bounces += 1;
            self.counters.bounce_ratio_sum += after / -p.relative_velocity;
            self.counters.restitution_sum += c.restitution;
        }
    }

    /// Puts each point's impulses back on its contact for the next tick, and
    /// raises the tick's [`KineticContact`]s.
    fn store(&mut self, scratch: &Scratch, bodies: &[RigidBody]) {
        let settings = self.settings;
        for c in &scratch.constraints {
            if let Some(Some(contact)) = self.contacts.get_mut(c.slot) {
                for (k, p) in c.points[..c.count].iter().enumerate() {
                    contact.impulses.normal[k] = p.normal_impulse;
                }
                contact.impulses.friction =
                    c.tangents[0] * c.tangent_impulse[0] + c.tangents[1] * c.tangent_impulse[1];
                contact.impulses.twist = c.twist_impulse;
            }

            let points = &c.points[..c.count];
            let approach = points
                .iter()
                .map(|p| -p.relative_velocity)
                .fold(f64::NEG_INFINITY, f64::max);
            let impulse: f64 = points.iter().map(|p| p.total_normal_impulse).sum();
            if approach < settings.restitution_threshold || impulse < settings.kinetic_impulse {
                continue;
            }

            let a_moves = c.a != NONE && c.mass_a.is_finite();
            let b_moves = c.b != NONE && c.mass_b.is_finite();
            let impactor_is_a = if !a_moves {
                true
            } else if !b_moves {
                false
            } else {
                c.velocity_a.dot(c.normal) >= -c.velocity_b.dot(c.normal)
            };
            let (impactor, struck, impactor_mass, normal, relative) = if impactor_is_a {
                (
                    c.entity_a,
                    c.entity_b,
                    c.mass_a,
                    c.normal,
                    -c.point_velocity,
                )
            } else {
                (
                    c.entity_b,
                    c.entity_a,
                    c.mass_b,
                    -c.normal,
                    c.point_velocity,
                )
            };
            let Some(struck) = struck else {
                continue;
            };
            let energy_deposited = points
                .iter()
                .map(|p| {
                    let after = relative_velocity(bodies, c, p).dot(c.normal);
                    let before = p.relative_velocity;
                    (0.5 * p.normal_mass * (before * before - after * after)).max(0.0)
                })
                .sum();
            let point = points.iter().map(|p| p.position).sum::<DVec3>() / c.count as f64;
            self.kinetic.push(KineticContact {
                source: KineticSource::Contact,
                impactor,
                struck,
                point,
                normal,
                relative_velocity: relative,
                impactor_mass,
                energy_deposited,
                impulse,
            });
        }
    }
}

/// Applies every contact's carried impulses: each point's normal impulse,
/// the friction at the centroid and the twist.
fn warm_start(constraints: &[Constraint], solver: &[SolverBody], bodies: &mut [RigidBody]) {
    for c in constraints {
        for p in &c.points[..c.count] {
            let impulse = c.normal * p.normal_impulse;
            apply(bodies, solver, c.a, p.anchor_a, -impulse);
            apply(bodies, solver, c.b, p.anchor_b, impulse);
        }
        let friction = c.tangents[0] * c.tangent_impulse[0] + c.tangents[1] * c.tangent_impulse[1];
        apply(bodies, solver, c.a, c.friction_anchor_a, -friction);
        apply(bodies, solver, c.b, c.friction_anchor_b, friction);
        apply_angular(bodies, solver, c.a, -c.normal * c.twist_impulse);
        apply_angular(bodies, solver, c.b, c.normal * c.twist_impulse);
    }
}

/// One pass over every constraint: the normal impulses, then friction.
///
/// `push_out` is `Some` for the biased solve — the soft spring, pushing at up
/// to that speed — and `None` for the relax, which is rigid and only stops
/// the bodies approaching.
fn solve_pass(
    constraints: &mut [Constraint],
    solver: &[SolverBody],
    bodies: &mut [RigidBody],
    h: f64,
    push_out: Option<f64>,
) {
    let inverse_h = 1.0 / h;
    for c in constraints.iter_mut() {
        let (dp_a, da_a) = motion_since_start(solver, c.a);
        let (dp_b, da_b) = motion_since_start(solver, c.b);

        for k in 0..c.count {
            let p = c.points[k];
            let moved = (dp_b - dp_a) + (da_b.cross(p.anchor_b) - da_a.cross(p.anchor_a));
            let separation = moved.dot(c.normal) + p.separation;
            let (bias, mass_scale, impulse_scale) = if separation > 0.0 {
                (separation * inverse_h, 1.0, 0.0)
            } else if let Some(push_out) = push_out {
                (
                    (c.softness.bias_rate * separation).max(-push_out),
                    c.softness.mass_scale,
                    c.softness.impulse_scale,
                )
            } else {
                (0.0, 1.0, 0.0)
            };
            let vn = relative_velocity(bodies, c, &p).dot(c.normal);
            let impulse =
                -p.normal_mass * mass_scale * (vn + bias) - impulse_scale * p.normal_impulse;
            let new = (p.normal_impulse + impulse).max(0.0);
            let delta = new - p.normal_impulse;
            let point = &mut c.points[k];
            point.normal_impulse = new;
            point.max_normal_impulse = point.max_normal_impulse.max(delta);
            apply(bodies, solver, c.a, p.anchor_a, -c.normal * delta);
            apply(bodies, solver, c.b, p.anchor_b, c.normal * delta);
        }

        // Friction at the centroid, bounded by the whole manifold's normal
        // impulse, then the twist, bounded by each point's share of it at its
        // distance from the centroid.
        let points = &c.points[..c.count];
        let dv = relative_velocity_at(bodies, c, c.friction_anchor_a, c.friction_anchor_b);
        let limit = c.friction * points.iter().map(|p| p.normal_impulse).sum::<f64>();
        let old = c.tangent_impulse;
        let mut new = [
            old[0] - c.tangent_mass[0] * dv.dot(c.tangents[0]),
            old[1] - c.tangent_mass[1] * dv.dot(c.tangents[1]),
        ];
        let length_squared = new[0] * new[0] + new[1] * new[1];
        if length_squared > limit * limit {
            let scale = limit / length_squared.sqrt();
            new = [new[0] * scale, new[1] * scale];
        }
        c.tangent_impulse = new;
        let impulse = c.tangents[0] * (new[0] - old[0]) + c.tangents[1] * (new[1] - old[1]);
        apply(bodies, solver, c.a, c.friction_anchor_a, -impulse);
        apply(bodies, solver, c.b, c.friction_anchor_b, impulse);

        // A one-point manifold's point stands for its patch: see the module
        // docs.
        let (_, wa) = velocity(bodies, c.a);
        let (_, wb) = velocity(bodies, c.b);
        let spin = (wb - wa).dot(c.normal);
        let limit = c.friction
            * points
                .iter()
                .map(|p| p.normal_impulse * p.twist_radius)
                .sum::<f64>();
        let old = c.twist_impulse;
        let new = (old - c.twist_mass * spin).clamp(-limit, limit);
        c.twist_impulse = new;
        apply_angular(bodies, solver, c.a, -c.normal * (new - old));
        apply_angular(bodies, solver, c.b, c.normal * (new - old));
    }
}

/// How far body `index` has moved and turned since the manifolds were built.
fn motion_since_start(solver: &[SolverBody], index: usize) -> (DVec3, DVec3) {
    if index == NONE {
        (DVec3::ZERO, DVec3::ZERO)
    } else {
        (solver[index].delta_position, solver[index].delta_angle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no stiffness the spring is off: full mass, no carried impulse,
    /// and no bias.
    #[test]
    fn a_zero_hertz_contact_is_rigid() {
        assert_eq!(
            Softness::new(0.0, 10.0, 1.0 / 240.0),
            Softness {
                bias_rate: 0.0,
                mass_scale: 1.0,
                impulse_scale: 0.0,
            }
        );
    }

    /// Box2D's coefficients for its defaults: a 30 Hz, damping-ratio-10
    /// contact at 240 Hz substeps keeps most of its mass and pushes out at
    /// roughly `ω / 2ζ` per metre.
    #[test]
    fn the_softness_of_the_defaults_matches_its_formula() {
        let h = 1.0 / 240.0;
        let soft = Softness::new(30.0, 10.0, h);
        let omega = TAU * 30.0;
        let a1 = 20.0 + h * omega;
        assert!((soft.bias_rate - omega / a1).abs() < 1e-12);
        let a2 = h * omega * a1;
        assert!((soft.mass_scale - a2 / (1.0 + a2)).abs() < 1e-12);
        assert!((soft.impulse_scale + soft.mass_scale - 1.0).abs() < 1e-12);
    }
}
