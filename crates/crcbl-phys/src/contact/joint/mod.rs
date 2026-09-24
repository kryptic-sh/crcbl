//! Joints in the Soft Step: contact-solver rung 5 (`docs/notes/simulation.md`),
//! L3's "a joint is a constraint with a different Jacobian".
//!
//! ```text
//!   step:  collide ─▶ joint events ─▶ islands ─▶ solve ─────────────────────▶ breaks
//!                     wake a sleeper   link the   prepare every joint with     take out
//!                     joined to what   joined     an awake dynamic body        each joint
//!                     moves; join      bodies'    ×substeps: warm start,       that
//!                     islands          islands    solve (biased), relax —      carried its
//!                                                 joints before contacts       threshold
//! ```
//!
//! Every joint type is transcribed from Box3D (github.com/erincatto/box3d,
//! commit `9e5a4cde`), the three-dimensional sibling of the Box2D v3 solver
//! decision 1 chose — each file here names its source — so a joint is
//! prepared, warm-started, solved with its soft bias and relaxed rigid
//! exactly as a contact is, in the same substeps, **joints before contacts**
//! as Box2D's overflow solve orders them. Its rigid rows are soft at the
//! joint's [`crate::Joint::constraint_hertz`], capped at a quarter of the
//! substep rate; limits are speculative, as a contact is, so a hinge swinging
//! into its stop is slowed before it gets there.
//!
//! # Anchors turn exactly
//!
//! A contact's anchors turn to first order within the tick, which suits a
//! rolling ball (see `solver.rs`). A joint's are turned through each body's
//! whole rotation since the tick began — Box3D's `deltaRotation` — because a
//! chain's links turn a long way and its anchors are what hold it together.
//!
//! # Breaking
//!
//! After each biased solve, a joint with a finite threshold reads its
//! reaction — Box3D's `b3GetJointReaction`, the impulses it carries over the
//! substep — and one that reached either threshold is marked. Box3D only
//! reports such a joint; here it is taken out at the end of the step and
//! reported as a [`crate::JointBreak`], which is decision 6's "breakable".

mod distance;
pub(crate) mod math;
mod prismatic;
mod revolute;
mod spherical;
mod weld;

use std::collections::HashMap;
use std::hash::Hasher;

use crcbl_core::{Handle, Pool};
use glam::{DMat3, DQuat, DVec3};

use super::solver::{NONE, Softness, SolverBody};
use super::{Bodies, IslandEvents, Presence, transform_of};
use crate::components::{RigidBody, Transform};
use crate::contact::island::IslandId;
use crate::joint::{Joint, JointBreak, JointDrift, JointId, JointKind};
use crate::system::{BodyId, BodyRecord, BodySet, canonical_bits};

/// A joint in the pool.
#[derive(Clone, Copy, Debug)]
pub(crate) struct JointRecord {
    pub(crate) joint: Joint,
    pub(crate) a: BodyId,
    pub(crate) b: BodyId,
    /// What it carries between ticks, for warm starting.
    impulses: Impulses,
    /// The force and torque it carried over the last substep it was solved,
    /// in newtons and newton-metres.
    reaction: (f64, f64),
}

type JointHandle = Handle<JointRecord>;

/// Every impulse any joint type carries, in N·s or N·m·s; each type reads
/// the ones it has.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Impulses {
    /// The point constraint's, or a weld's linear half.
    linear: DVec3,
    /// A weld's or a slider's rotation lock.
    angular: DVec3,
    /// A hinge's two collinearity rows, or a slider's two perpendicular ones.
    perp: [f64; 2],
    /// A distance joint's length.
    axial: f64,
    spring: f64,
    motor: f64,
    lower: f64,
    upper: f64,
    /// A spherical joint's motor.
    motor_vector: DVec3,
    /// A spherical joint's cone.
    swing: f64,
}

impl Impulses {
    fn values(&self) -> [f64; 17] {
        [
            self.linear.x,
            self.linear.y,
            self.linear.z,
            self.angular.x,
            self.angular.y,
            self.angular.z,
            self.perp[0],
            self.perp[1],
            self.axial,
            self.spring,
            self.motor,
            self.lower,
            self.upper,
            self.motor_vector.x,
            self.motor_vector.y,
            self.motor_vector.z,
            self.swing,
        ]
    }
}

/// A joint frame in the world, at the tick's start: its orientation, and
/// its anchor from the body's centre.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) p: DVec3,
    pub(super) q: DQuat,
}

/// What the kinds prepare beyond the common part.
#[derive(Clone, Copy, Debug)]
pub(super) enum Kind {
    Distance(distance::Prepared),
    Revolute(revolute::Prepared),
    Prismatic(prismatic::Prepared),
    Weld(weld::Prepared),
    Spherical(spherical::Prepared),
}

/// One joint, prepared for a tick: Box3D's `b3JointSim` after
/// `b3PrepareJointsTask`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Prepared {
    handle: JointHandle,
    /// Each side's awake index, or [`NONE`] for one that does not step.
    pub(super) a: usize,
    pub(super) b: usize,
    pub(super) ma: f64,
    pub(super) mb: f64,
    pub(super) ia: DMat3,
    pub(super) ib: DMat3,
    pub(super) fixed_rotation: bool,
    pub(super) frame_a: Frame,
    pub(super) frame_b: Frame,
    /// Body B's centre less body A's, at the tick's start.
    pub(super) delta_center: DVec3,
    /// The rigid rows' softness for the substep being solved.
    pub(super) softness: Softness,
    pub(super) impulses: Impulses,
    pub(super) kind: Kind,
    joint: Joint,
    /// The force and torque that broke it, if it broke this tick.
    broke: Option<(f64, f64)>,
    reaction: (f64, f64),
}

impl Prepared {
    /// Recomputes every softness for a substep of `h`, the rigid rows' at
    /// the joint's own stiffness times `scale` — see
    /// [`crate::PhysicsSystem::set_substeps`] — capped at a quarter of the
    /// substep rate, as Box3D's `b3PrepareJoint` caps it.
    pub(super) fn soften(&mut self, h: f64, scale: f64) {
        let hertz = (self.joint.constraint_hertz * scale).min(0.25 / h);
        self.softness = math::make_soft(hertz, self.joint.constraint_damping_ratio, h);
        let softness = self.softness;
        match &mut self.kind {
            Kind::Distance(d) => d.soften(h),
            Kind::Revolute(r) => r.soften(h),
            Kind::Prismatic(p) => p.soften(h),
            Kind::Weld(w) => w.soften(h, softness),
            Kind::Spherical(_) => {}
        }
    }

    /// The awake indices of its dynamic sides: what ties it to a solver
    /// group.
    pub(super) fn sides(&self) -> (usize, usize) {
        (self.a, self.b)
    }
}

/// A body's motion since the tick began: how far its centre moved and how
/// it turned — identity for a side that does not step.
pub(super) fn motion(solver: &[SolverBody], index: usize) -> (DVec3, DQuat) {
    if index == NONE {
        (DVec3::ZERO, DQuat::IDENTITY)
    } else {
        (solver[index].delta_position, solver[index].delta_rotation)
    }
}

/// A joint's two bodies' velocities while it is solved: Box3D's local
/// `vA`, `wA`, `vB`, `wB`, read from the bodies — zero for a side that does
/// not step — and written back to the dynamic ones.
#[derive(Clone, Copy, Debug)]
pub(super) struct Velocities {
    pub(super) va: DVec3,
    pub(super) wa: DVec3,
    pub(super) vb: DVec3,
    pub(super) wb: DVec3,
}

impl Velocities {
    pub(super) fn read(joint: &Prepared, bodies: &[RigidBody]) -> Self {
        let of = |index: usize| {
            if index == NONE {
                (DVec3::ZERO, DVec3::ZERO)
            } else {
                (bodies[index].velocity, bodies[index].angular_velocity)
            }
        };
        let ((va, wa), (vb, wb)) = (of(joint.a), of(joint.b));
        Self { va, wa, vb, wb }
    }

    /// Writes them back to the sides that are dynamic: Box3D's
    /// `b3_dynamicFlag` test.
    pub(super) fn write(self, joint: &Prepared, bodies: &mut [RigidBody]) {
        for (index, v, w) in [(joint.a, self.va, self.wa), (joint.b, self.vb, self.wb)] {
            if index != NONE && bodies[index].is_dynamic() {
                bodies[index].velocity = v;
                bodies[index].angular_velocity = w;
            }
        }
    }

    /// The linear impulse `p` at anchors `ra` and `rb`: minus to A, plus to
    /// B.
    pub(super) fn push(&mut self, joint: &Prepared, p: DVec3, ra: DVec3, rb: DVec3) {
        self.push_turning(joint, p, ra.cross(p), rb.cross(p));
    }

    /// The linear impulse `p` with the angular impulses `la` on A and `lb`
    /// on B that go with it: minus to A, plus to B.
    pub(super) fn push_turning(&mut self, joint: &Prepared, p: DVec3, la: DVec3, lb: DVec3) {
        self.va -= joint.ma * p;
        self.wa -= joint.ia * la;
        self.vb += joint.mb * p;
        self.wb += joint.ib * lb;
    }

    /// The angular impulse `l`: minus to A, plus to B.
    pub(super) fn turn(&mut self, joint: &Prepared, l: DVec3) {
        self.wa -= joint.ia * l;
        self.wb += joint.ib * l;
    }

    /// B's point at `rb` moving less A's at `ra`.
    pub(super) fn relative(&self, ra: DVec3, rb: DVec3) -> DVec3 {
        self.vb + self.wb.cross(rb) - self.va - self.wa.cross(ra)
    }
}

/// A limit row's bias and scales, for a gap `c` — positive while the limit
/// holds: speculative while open, soft while closed in a biased solve, and
/// rigid in the relax. Box3D writes this out in every limit it has.
pub(super) fn limit_bias(
    c: f64,
    inverse_h: f64,
    softness: Softness,
    use_bias: bool,
) -> (f64, f64, f64) {
    if c > 0.0 {
        (c * inverse_h, 1.0, 0.0)
    } else if use_bias {
        (
            softness.bias_rate * c,
            softness.mass_scale,
            softness.impulse_scale,
        )
    } else {
        (0.0, 1.0, 0.0)
    }
}

/// Every joint of a system with contacts.
#[derive(Debug, Default)]
pub(crate) struct Joints {
    pool: Pool<JointRecord>,
    /// Pairs of bodies joined by a joint that keeps them from colliding, by
    /// their ids' bits, lower first, with how many such joints join them.
    /// Only ever looked up, so its order never matters.
    jointed: HashMap<(u64, u64), u32>,
    /// This tick's prepared joints.
    pub(super) prepared: Vec<Prepared>,
    /// Joints that broke this step, taken out by the system.
    pub(crate) broken: Vec<JointBreak>,
}

fn pair_key(a: BodyId, b: BodyId) -> (u64, u64) {
    let (a, b) = (a.to_bits(), b.to_bits());
    (a.min(b), a.max(b))
}

impl Joints {
    /// How many joints there are.
    pub(crate) fn len(&self) -> usize {
        self.pool.len()
    }

    /// Adds a joint between bodies `a` and `b`.
    pub(crate) fn insert(&mut self, joint: Joint, a: BodyId, b: BodyId) -> JointId {
        if !joint.collide_connected {
            *self.jointed.entry(pair_key(a, b)).or_insert(0) += 1;
        }
        JointId(self.pool.insert(JointRecord {
            joint,
            a,
            b,
            impulses: Impulses::default(),
            reaction: (0.0, 0.0),
        }))
    }

    /// Takes a joint out.
    pub(crate) fn remove(&mut self, id: JointId) -> Option<JointRecord> {
        let record = self.pool.remove(id.0)?;
        self.unkeep(&record);
        Some(record)
    }

    /// Replaces a joint's settings, keeping what it carries if it is still
    /// the same kind between the same bodies.
    pub(crate) fn replace(&mut self, id: JointId, joint: Joint, a: BodyId, b: BodyId) -> bool {
        let Some(old) = self.pool.get(id.0).copied() else {
            return false;
        };
        self.unkeep(&old);
        if !joint.collide_connected {
            *self.jointed.entry(pair_key(a, b)).or_insert(0) += 1;
        }
        let same = old.a == a
            && old.b == b
            && core::mem::discriminant(&old.joint.kind) == core::mem::discriminant(&joint.kind);
        let record = self.pool.get_mut(id.0).expect("read above");
        *record = JointRecord {
            joint,
            a,
            b,
            impulses: if same {
                old.impulses
            } else {
                Impulses::default()
            },
            reaction: if same { old.reaction } else { (0.0, 0.0) },
        };
        true
    }

    /// Forgets that `record` keeps its bodies apart.
    fn unkeep(&mut self, record: &JointRecord) {
        if record.joint.collide_connected {
            return;
        }
        let key = pair_key(record.a, record.b);
        if let Some(count) = self.jointed.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.jointed.remove(&key);
            }
        }
    }

    /// The joint `id` names.
    pub(crate) fn get(&self, id: JointId) -> Option<&JointRecord> {
        self.pool.get(id.0)
    }

    /// Whether a joint keeps bodies `a` and `b` from colliding.
    pub(crate) fn keeps_apart(&self, a: BodyId, b: BodyId) -> bool {
        !self.jointed.is_empty() && self.jointed.contains_key(&pair_key(a, b))
    }

    /// Every joint body `id` is part of, in pool order.
    pub(crate) fn of_body(&self, id: BodyId) -> Vec<JointId> {
        self.pool
            .iter()
            .filter(|(_, record)| record.a == id || record.b == id)
            .map(|(handle, _)| JointId(handle))
            .collect()
    }

    /// Every joint, in pool order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (JointId, &JointRecord)> {
        self.pool
            .iter()
            .map(|(handle, record)| (JointId(handle), record))
    }

    /// What the joints ask of the islands this step, into `events`: a
    /// sleeping body joined to one that moves wakes, two island bodies joined
    /// are linked, and a dynamic body joined to a moving kinematic one is not
    /// still.
    pub(crate) fn island_events(&self, bodies: Bodies<'_>, events: &mut IslandEvents) {
        for (_, record) in self.pool.iter() {
            let (Some(pa), Some(pb)) = (
                Presence::of(bodies, record.a),
                Presence::of(bodies, record.b),
            ) else {
                continue;
            };
            for (this, other) in [(pa, pb), (pb, pa)] {
                match (this, other) {
                    (Presence::Asleep(id), other) if other.moves() => events.wake.push(id),
                    (Presence::Dynamic(id), Presence::Moving) => events.stirred.push(id),
                    _ => {}
                }
            }
            if let (Some(a), Some(b)) = (pa.island_body(), pb.island_body()) {
                events.links.push((a, b));
            }
        }
    }

    /// Every joint between two bodies of island `island`, into `edges`.
    pub(crate) fn island_edges(
        &self,
        records: &Pool<BodyRecord>,
        island: IslandId,
        edges: &mut Vec<(BodyId, BodyId)>,
    ) {
        let in_island = |id: BodyId| {
            records
                .get(id)
                .is_some_and(|record| record.island == Some(island))
        };
        for (_, record) in self.pool.iter() {
            if in_island(record.a) && in_island(record.b) {
                edges.push((record.a, record.b));
            }
        }
    }

    /// Prepares every joint one of whose bodies is awake and dynamic, for
    /// substeps of `h`.
    pub(super) fn prepare(
        &mut self,
        bodies: Bodies<'_>,
        solver: &[SolverBody],
        h: f64,
        warm_starting: bool,
    ) {
        self.prepared.clear();
        self.broken.clear();
        for (handle, record) in self.pool.iter() {
            let dynamic = |id| matches!(Presence::of(bodies, id), Some(Presence::Dynamic(_)));
            if !dynamic(record.a) && !dynamic(record.b) {
                continue;
            }
            let (Some(a), Some(b)) = (
                Side::of(bodies, solver, record.a),
                Side::of(bodies, solver, record.b),
            ) else {
                continue;
            };
            let joint = record.joint;
            let frame = |side: &Side<'_>, local: &Transform| Frame {
                q: side.transform.rotation * local.rotation,
                p: side.transform.rotation * local.position,
            };
            let frame_a = frame(&a, &joint.frame_a);
            let frame_b = frame(&b, &joint.frame_b);
            let delta_center = b.transform.position - a.transform.position;
            let fixed_rotation = math::is_fixed_rotation(a.inverse_inertia + b.inverse_inertia);
            let common = Common {
                ma: a.inverse_mass,
                mb: b.inverse_mass,
                ia: a.inverse_inertia,
                ib: b.inverse_inertia,
                frame_a,
                frame_b,
                delta_center,
            };
            let kind = match joint.kind {
                JointKind::Distance(d) => Kind::Distance(distance::Prepared::new(d, &common)),
                JointKind::Revolute(r) => Kind::Revolute(revolute::Prepared::new(r, &common)),
                JointKind::Prismatic(p) => Kind::Prismatic(prismatic::Prepared::new(p, &common)),
                JointKind::Weld(w) => Kind::Weld(weld::Prepared::new(w, &common)),
                JointKind::Spherical(s) => Kind::Spherical(spherical::Prepared::new(s, &common)),
            };
            let mut prepared = Prepared {
                handle,
                a: a.index,
                b: b.index,
                ma: common.ma,
                mb: common.mb,
                ia: common.ia,
                ib: common.ib,
                fixed_rotation,
                frame_a,
                frame_b,
                delta_center,
                softness: Softness::new(0.0, 0.0, h),
                impulses: if warm_starting {
                    record.impulses
                } else {
                    Impulses::default()
                },
                kind,
                joint,
                broke: None,
                reaction: (0.0, 0.0),
            };
            prepared.soften(h, 1.0);
            self.prepared.push(prepared);
        }
    }

    /// Stores every prepared joint's impulses and reaction back on its
    /// record, and lists the ones that broke.
    pub(crate) fn store(&mut self, records: &Pool<BodyRecord>) {
        for prepared in &self.prepared {
            let Some(record) = self.pool.get_mut(prepared.handle) else {
                continue;
            };
            record.impulses = prepared.impulses;
            record.reaction = prepared.reaction;
            if let Some((force, torque)) = prepared.broke {
                let entity = |id| records.get(id).map(|r: &BodyRecord| r.entity);
                if let (Some(body_a), Some(body_b)) = (entity(record.a), entity(record.b)) {
                    self.broken.push(JointBreak {
                        joint: JointId(prepared.handle),
                        body_a,
                        body_b,
                        joint_def: record.joint,
                        force,
                        torque,
                    });
                }
            }
        }
    }

    /// The force and torque joint `id` carried over the last substep it was
    /// solved in.
    pub(crate) fn reaction(&self, id: JointId) -> Option<(f64, f64)> {
        self.pool.get(id.0).map(|record| record.reaction)
    }

    /// Feeds every joint into a determinism hash, in pool order, which is
    /// what names them: their ids, their bodies' entities, their settings and
    /// what they carry. Nothing is written when there are none, so a system
    /// with no joints hashes as it did before joints.
    pub(crate) fn hash_state(&self, records: &Pool<BodyRecord>, hasher: &mut dyn Hasher) {
        if self.pool.is_empty() {
            return;
        }
        let write = |hasher: &mut dyn Hasher, value: f64| {
            hasher.write(&canonical_bits(value).to_le_bytes());
        };
        hasher.write(&[4]);
        hasher.write(&(self.pool.len() as u64).to_le_bytes());
        for (handle, record) in self.pool.iter() {
            hasher.write(&handle.to_bits().to_le_bytes());
            for id in [record.a, record.b] {
                let bits = records.get(id).map_or(0, |r| r.entity.to_bits());
                hasher.write(&bits.to_le_bytes());
            }
            let joint = &record.joint;
            for frame in [joint.frame_a, joint.frame_b] {
                for value in frame.position.to_array() {
                    write(hasher, value);
                }
                for value in frame.rotation.to_array() {
                    write(hasher, value);
                }
            }
            hasher.write(&[u8::from(joint.collide_connected)]);
            for value in [
                joint.force_threshold,
                joint.torque_threshold,
                joint.constraint_hertz,
                joint.constraint_damping_ratio,
            ] {
                write(hasher, value);
            }
            let (tag, flags, values) = settings(&joint.kind);
            hasher.write(&[tag, flags]);
            for value in values {
                write(hasher, value);
            }
            for value in record.impulses.values() {
                write(hasher, value);
            }
        }
    }
}

/// A joint kind's settings for the hash: a tag, its switches as bits, and
/// its numbers.
fn settings(kind: &JointKind) -> (u8, u8, Vec<f64>) {
    let bits = |flags: &[bool]| {
        flags
            .iter()
            .enumerate()
            .fold(0u8, |acc, (k, &on)| acc | (u8::from(on) << k))
    };
    match kind {
        JointKind::Distance(d) => (
            0,
            bits(&[d.enable_spring, d.enable_limit, d.enable_motor]),
            vec![
                d.length,
                d.spring.hertz,
                d.spring.damping_ratio,
                d.min_length,
                d.max_length,
                d.max_motor_force,
                d.motor_speed,
            ],
        ),
        JointKind::Revolute(r) => (
            1,
            bits(&[r.enable_spring, r.enable_limit, r.enable_motor]),
            vec![
                r.spring.hertz,
                r.spring.damping_ratio,
                r.target_angle,
                r.lower_angle,
                r.upper_angle,
                r.max_motor_torque,
                r.motor_speed,
            ],
        ),
        JointKind::Prismatic(p) => (
            2,
            bits(&[p.enable_spring, p.enable_limit, p.enable_motor]),
            vec![
                p.spring.hertz,
                p.spring.damping_ratio,
                p.target_translation,
                p.lower_translation,
                p.upper_translation,
                p.max_motor_force,
                p.motor_speed,
            ],
        ),
        JointKind::Weld(w) => (
            3,
            0,
            vec![
                w.linear.hertz,
                w.linear.damping_ratio,
                w.angular.hertz,
                w.angular.damping_ratio,
            ],
        ),
        JointKind::Spherical(s) => (
            4,
            bits(&[s.enable_cone_limit, s.enable_twist_limit, s.enable_motor]),
            vec![
                s.cone_angle,
                s.lower_twist_angle,
                s.upper_twist_angle,
                s.max_motor_torque,
                s.motor_velocity.x,
                s.motor_velocity.y,
                s.motor_velocity.z,
            ],
        ),
    }
}

/// What every kind's `new` reads.
pub(super) struct Common {
    pub(super) ma: f64,
    pub(super) mb: f64,
    pub(super) ia: DMat3,
    pub(super) ib: DMat3,
    pub(super) frame_a: Frame,
    pub(super) frame_b: Frame,
    pub(super) delta_center: DVec3,
}

/// One side of a joint, resolved for a tick.
struct Side<'a> {
    index: usize,
    inverse_mass: f64,
    inverse_inertia: DMat3,
    transform: &'a Transform,
}

impl<'a> Side<'a> {
    fn of(bodies: Bodies<'a>, solver: &[SolverBody], id: BodyId) -> Option<Self> {
        let record = bodies.records.get(id)?;
        let transform = transform_of(record, bodies);
        Some(match record.set {
            BodySet::Awake => {
                let s = &solver[record.index];
                Self {
                    index: record.index,
                    inverse_mass: s.inverse_mass,
                    inverse_inertia: s.inverse_inertia,
                    transform,
                }
            }
            // A static body is an anchor; a sleeping one is only met here if
            // its island failed to wake, and is held still like one.
            BodySet::Static | BodySet::Sleeping => Self {
                index: NONE,
                inverse_mass: 0.0,
                inverse_inertia: DMat3::ZERO,
                transform,
            },
        })
    }
}

/// Warm-starts the prepared joints: Box3D's `b3WarmStartJoint`. The angular
/// velocity each one gives its bodies is added to `carried`, by awake index,
/// for the step to turn them by in full — see
/// [`crate::SemiImplicitEuler::integrate_position_carrying`].
pub(super) fn warm_start(
    joints: &[Prepared],
    solver: &[SolverBody],
    bodies: &mut [RigidBody],
    carried: &mut [DVec3],
) {
    for joint in joints {
        let before = Velocities::read(joint, bodies);
        match &joint.kind {
            Kind::Distance(d) => d.warm_start(joint, solver, bodies),
            Kind::Revolute(r) => r.warm_start(joint, solver, bodies),
            Kind::Prismatic(p) => p.warm_start(joint, solver, bodies),
            Kind::Weld(_) => weld::warm_start(joint, solver, bodies),
            Kind::Spherical(s) => s.warm_start(joint, solver, bodies),
        }
        carry(joint, before, bodies, carried);
    }
}

/// Adds what a joint just did to its bodies' angular velocities, since
/// `before`, to `carried`.
fn carry(joint: &Prepared, before: Velocities, bodies: &[RigidBody], carried: &mut [DVec3]) {
    let after = Velocities::read(joint, bodies);
    for (index, from, to) in [
        (joint.a, before.wa, after.wa),
        (joint.b, before.wb, after.wb),
    ] {
        if index != NONE {
            carried[index] += to - from;
        }
    }
}

/// Solves the prepared joints in turn: Box3D's `b3SolveJoint`, and after a
/// biased solve its threshold test from `b3SolveJointsTask`. A biased
/// solve's angular changes are added to `carried`, as the warm start's are;
/// the relax comes after the step and carries nothing.
pub(super) fn solve(
    joints: &mut [Prepared],
    solver: &[SolverBody],
    bodies: &mut [RigidBody],
    h: f64,
    mut carried: Option<&mut [DVec3]>,
) {
    let use_bias = carried.is_some();
    let inverse_h = 1.0 / h;
    for joint in joints.iter_mut() {
        let before = Velocities::read(joint, bodies);
        let mut kind = joint.kind;
        match &mut kind {
            Kind::Distance(d) => d.solve(joint, solver, bodies, h, use_bias),
            Kind::Revolute(r) => r.solve(joint, solver, bodies, h, use_bias),
            Kind::Prismatic(p) => p.solve(joint, solver, bodies, h, use_bias),
            Kind::Weld(w) => w.solve(joint, solver, bodies, use_bias),
            Kind::Spherical(s) => s.solve(joint, solver, bodies, h, use_bias),
        }
        joint.kind = kind;
        if let Some(carried) = carried.as_deref_mut() {
            carry(joint, before, bodies, carried);
        }
        let (linear, angular) = reaction_impulses(joint);
        joint.reaction = (linear * inverse_h, angular * inverse_h);
        if use_bias
            && joint.broke.is_none()
            && (joint.reaction.0 >= joint.joint.force_threshold
                || joint.reaction.1 >= joint.joint.torque_threshold)
        {
            joint.broke = Some(joint.reaction);
        }
    }
}

/// The linear and angular impulse a joint carries: Box3D's
/// `b3GetJointReaction`, before its division by the substep.
///
/// A spherical joint's angular impulse is summed from the axes it was
/// applied along, as its warm start applies it, where Box3D recomputes the
/// swing and twist axes from the bodies' transforms.
fn reaction_impulses(joint: &Prepared) -> (f64, f64) {
    let i = &joint.impulses;
    match &joint.kind {
        Kind::Distance(_) => ((i.axial + i.lower - i.upper + i.motor).abs(), 0.0),
        Kind::Revolute(_) => (
            i.linear.length(),
            DVec3::new(i.perp[0], i.perp[1], i.motor + i.lower - i.upper).length(),
        ),
        Kind::Prismatic(_) => (
            DVec3::new(i.motor + i.lower - i.upper, i.perp[0], i.perp[1]).length(),
            i.angular.length(),
        ),
        Kind::Weld(_) => (i.linear.length(), i.angular.length()),
        Kind::Spherical(s) => (i.linear.length(), s.angular_impulse(i).length()),
    }
}

/// How far `joint`, with its bodies at `a` and `b`, is from holding: see
/// [`JointDrift`].
pub(crate) fn drift(joint: &Joint, a: &Transform, b: &Transform) -> JointDrift {
    let qa = a.rotation * joint.frame_a.rotation;
    let mut qb = b.rotation * joint.frame_b.rotation;
    if qa.dot(qb) < 0.0 {
        qb = -qb;
    }
    let d = (b.position + b.rotation * joint.frame_b.position)
        - (a.position + a.rotation * joint.frame_a.position);
    let rel = math::inv_mul(qa, qb);
    let turned = 2.0 * math::atan2(DVec3::new(rel.x, rel.y, rel.z).length(), rel.w.abs());
    let outside = |value: f64, lower: f64, upper: f64| (lower - value).max(value - upper).max(0.0);
    match &joint.kind {
        JointKind::Distance(dj) => {
            let length = d.length();
            let soft = dj.enable_spring && (dj.min_length < dj.max_length || !dj.enable_limit);
            let linear = if !soft {
                (length - dj.length).abs()
            } else if dj.enable_limit {
                outside(length, dj.min_length, dj.max_length)
            } else {
                0.0
            };
            JointDrift {
                linear,
                angular: 0.0,
            }
        }
        JointKind::Revolute(r) => {
            let mut angular = math::swing_angle(rel);
            if r.enable_limit {
                angular = angular.max(outside(
                    math::twist_angle(rel),
                    r.lower_angle,
                    r.upper_angle,
                ));
            }
            JointDrift {
                linear: d.length(),
                angular,
            }
        }
        JointKind::Prismatic(p) => {
            let axis = qa * DVec3::X;
            let along = d.dot(axis);
            let mut linear = (d - axis * along).length();
            if p.enable_limit {
                linear = linear.max(outside(along, p.lower_translation, p.upper_translation));
            }
            JointDrift {
                linear,
                angular: turned,
            }
        }
        JointKind::Weld(_) => JointDrift {
            linear: d.length(),
            angular: turned,
        },
        JointKind::Spherical(s) => {
            let mut angular: f64 = 0.0;
            if s.enable_cone_limit {
                angular = angular.max(math::swing_angle(rel) - s.cone_angle);
            }
            if s.enable_twist_limit {
                angular = angular.max(outside(
                    math::twist_angle(rel),
                    s.lower_twist_angle,
                    s.upper_twist_angle,
                ));
            }
            JointDrift {
                linear: d.length(),
                angular,
            }
        }
    }
}

impl Presence {
    /// How body `id` takes part in a tick: [`super::ContactPipeline`]'s
    /// reading of a proxy's owner, for a body named directly.
    pub(super) fn of(bodies: Bodies<'_>, id: BodyId) -> Option<Self> {
        super::body_presence(bodies, id)
    }
}
