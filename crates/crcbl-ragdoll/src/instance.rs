//! A running ragdoll: a rig's bodies and joints in a physics system, seeded
//! from the last animated pose, and read back as a skeletal pose.
//!
//! What stays with the caller: choosing the entities, deciding the death,
//! picking which contact is the killing one, and what a settled corpse is for.

use crcbl_anim::ragdoll::{BodyJoint, RagdollBinding, RagdollError};
use crcbl_anim::{Pose, Skeleton};
use crcbl_ecs::Entity;
use crcbl_phys::contact::manifold::collide;
use crcbl_phys::contact::shape::ContactShape;
use crcbl_phys::{
    ImpulseError, Joint, JointError, JointId, KineticContact, MassProperties, PhysicsSystem,
    RigidBody, Transform,
};
use glam::{DVec3, Mat4};

use crate::frames::{body_worlds, rigid_frames};
use crate::rig::RagdollRig;

/// A pose sampled shortly before the handoff, from which the bodies' initial
/// velocities are inferred.
#[derive(Clone, Copy, Debug)]
pub struct PreviousPose<'a> {
    /// The pose.
    pub pose: &'a Pose,
    /// Where it was placed in the world.
    pub model: Mat4,
    /// Seconds from it to the handoff pose: finite and positive.
    pub seconds: f64,
}

/// The animated state a ragdoll takes over from.
#[derive(Clone, Copy, Debug)]
pub struct Handoff<'a> {
    /// The last animated pose.
    pub pose: &'a Pose,
    /// Where it is placed in the world, at the scale the rig was made for.
    pub model: Mat4,
    /// An earlier pose to infer velocities from; the bodies start still
    /// without one.
    pub previous: Option<PreviousPose<'a>>,
}

/// Why a ragdoll was not spawned, or could not do what was asked.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SpawnError {
    /// The rig was made for a skeleton of another size.
    #[error("the rig is for {rig} joints and the skeleton has {skeleton}")]
    SkeletonSize {
        /// The rig's joint count.
        rig: usize,
        /// The skeleton's.
        skeleton: usize,
    },
    /// One entity per rig body is needed.
    #[error("{entities} entities given for {bodies} rig bodies")]
    EntityCount {
        /// How many were given.
        entities: usize,
        /// How many bodies.
        bodies: usize,
    },
    /// An entity is already registered with the system, or given twice.
    #[error("{0:?} is already in use")]
    EntityInUse(Entity),
    /// The handoff pose leaves a joint with no usable frame.
    #[error("skeleton joint {joint} has no usable frame at the handoff")]
    InvalidFrame {
        /// The joint.
        joint: usize,
    },
    /// The pose bridge refused the handoff.
    #[error(transparent)]
    Binding(#[from] RagdollError),
    /// The system refused a joint; nothing was left behind.
    #[error(transparent)]
    Joint(#[from] JointError),
}

/// Why [`Ragdoll::strike_with`] did not apply a contact.
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum StrikeError {
    /// The contact struck one of this ragdoll's own bodies, so the solver
    /// has already applied its impulse.
    #[error("the contact struck the ragdoll itself, and was already applied")]
    AlreadyApplied,
    /// A body of this ragdoll is no longer in the system.
    #[error("a ragdoll body is no longer in the physics system")]
    BodyMissing,
    /// The impulse or point was refused.
    #[error(transparent)]
    Impulse(#[from] ImpulseError),
}

/// A rig's bodies and joints, live in a physics system.
///
/// Drop does not remove anything from the system: call
/// [`remove`](Self::remove).
#[derive(Debug)]
pub struct Ragdoll {
    pub(crate) entities: Vec<Entity>,
    /// Each body's skeleton joint, in rig order.
    pub(crate) skeleton_joints: Vec<usize>,
    /// Where each body's joint origin sits in the body's frame: the inverse
    /// of the rig body's offset, applied to the origin.
    pub(crate) joint_in_body: Vec<DVec3>,
    joints: Vec<JointId>,
    shapes: Vec<crcbl_phys::ColliderComponent>,
    binding: RagdollBinding,
    model: Mat4,
    worlds: Vec<Mat4>,
    /// Seconds awake without a break: see `SettleBudget`.
    pub(crate) awake_for: f64,
}

impl Ragdoll {
    /// Creates `rig`'s bodies on `entities` (one per rig body, none yet in
    /// `physics`) where the handoff pose places them, with velocities from
    /// the previous pose if one is given, then its joints and ignored pairs.
    ///
    /// Everything is checked before the system is touched; a joint the system
    /// still refuses takes the bodies back out again.
    ///
    /// # Errors
    ///
    /// [`SpawnError`] for a mismatched rig, skeleton, entity list or pose.
    ///
    /// # Panics
    ///
    /// Panics if `physics` has no contacts: joints are contact-solver
    /// constraints.
    pub fn spawn(
        physics: &mut PhysicsSystem,
        rig: &RagdollRig,
        skeleton: &Skeleton,
        handoff: &Handoff<'_>,
        entities: &[Entity],
    ) -> Result<Self, SpawnError> {
        if rig.joint_count() != skeleton.len() {
            return Err(SpawnError::SkeletonSize {
                rig: rig.joint_count(),
                skeleton: skeleton.len(),
            });
        }
        if entities.len() != rig.bodies().len() {
            return Err(SpawnError::EntityCount {
                entities: entities.len(),
                bodies: rig.bodies().len(),
            });
        }
        for (index, &entity) in entities.iter().enumerate() {
            if physics.transform(entity).is_some() || entities[..index].contains(&entity) {
                return Err(SpawnError::EntityInUse(entity));
            }
        }
        let frames = rigid_frames(skeleton, handoff.pose, handoff.model)
            .map_err(|joint| SpawnError::InvalidFrame { joint })?;
        let worlds = body_worlds(rig.bodies(), &frames);
        let bound: Vec<BodyJoint> = rig
            .bodies()
            .iter()
            .zip(&worlds)
            .map(|(body, world)| BodyJoint {
                joint: body.joint,
                world: matrix(world),
            })
            .collect();
        let binding = RagdollBinding::new(skeleton, handoff.pose, handoff.model, &bound)?;
        let motion = handoff
            .previous
            .map(|previous| {
                binding.motion_from_previous_pose(previous.pose, previous.model, previous.seconds)
            })
            .transpose()?;

        let shapes: Vec<_> = rig
            .bodies()
            .iter()
            .map(|body| body.shape.collider())
            .collect();
        for (index, body) in rig.bodies().iter().enumerate() {
            let entity = entities[index];
            let inertia = MassProperties::of_collider(&shapes[index], body.mass).inertia;
            let mut dynamic = RigidBody::new_dynamic(body.mass).with_inertia(inertia);
            if let Some(motion) = &motion {
                dynamic.velocity = motion[index].linear_velocity;
                dynamic.angular_velocity = motion[index].angular_velocity;
            }
            physics.set_body(entity, dynamic);
            physics.set_transform(entity, worlds[index]);
            physics.set_collider(entity, &shapes[index], &worlds[index]);
            if rig.substeps() != 0 {
                physics.set_substeps(entity, rig.substeps());
            }
        }
        let mut joints = Vec::with_capacity(rig.joints().len());
        for joint in rig.joints() {
            let added = physics.add_joint(Joint::new(
                entities[joint.parent],
                entities[joint.child],
                joint.frame_parent,
                joint.frame_child,
                joint.kind.joint_kind(joint.friction),
            ));
            match added {
                Ok(id) => joints.push(id),
                Err(error) => {
                    for &entity in entities {
                        physics.remove_entity(entity);
                    }
                    return Err(error.into());
                }
            }
        }
        for &(a, b) in rig.ignored() {
            physics
                .ignore_contacts(entities[a], entities[b])
                .expect("both bodies were registered above");
        }
        Ok(Self {
            entities: entities.to_vec(),
            skeleton_joints: rig.bodies().iter().map(|body| body.joint).collect(),
            joint_in_body: rig
                .bodies()
                .iter()
                .map(|body| -(body.offset.rotation.inverse() * body.offset.position))
                .collect(),
            joints,
            shapes,
            binding,
            model: handoff.model,
            worlds: vec![Mat4::IDENTITY; entities.len()],
            awake_for: 0.0,
        })
    }

    /// The bodies' entities, in rig order.
    #[must_use]
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The joints, in rig order. One that broke is no longer in the system.
    #[must_use]
    pub fn joints(&self) -> &[JointId] {
        &self.joints
    }

    /// The model transform the ragdoll was spawned with: the one to read the
    /// pose back with while the skeleton's roots are driven by bodies.
    #[must_use]
    pub const fn handoff_model(&self) -> Mat4 {
        self.model
    }

    /// The body whose surface is nearest `point`, by rig index, and its
    /// signed distance from it: negative inside. `None` if a body is gone.
    #[must_use]
    pub fn nearest_body(&self, physics: &PhysicsSystem, point: DVec3) -> Option<(usize, f64)> {
        let probe = ContactShape::Sphere {
            centre: point,
            radius: 0.0,
        };
        let mut nearest: Option<(usize, f64)> = None;
        for (index, (&entity, collider)) in self.entities.iter().zip(&self.shapes).enumerate() {
            let shape = ContactShape::placed(collider, physics.transform(entity)?)?;
            let distance = collide(&probe, &shape, f64::MAX)
                .points()
                .iter()
                .map(|contact| contact.separation)
                .reduce(f64::min)?;
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((index, distance));
            }
        }
        nearest
    }

    /// Applies `impulse` at `point`, both world-space, to the body nearest
    /// the point, and returns its rig index: the killing blow, thrown into
    /// the limb it landed on.
    ///
    /// # Errors
    ///
    /// [`StrikeError::BodyMissing`] if a body is gone, and
    /// [`StrikeError::Impulse`] for an impulse the system refuses.
    pub fn strike(
        &self,
        physics: &mut PhysicsSystem,
        impulse: DVec3,
        point: DVec3,
    ) -> Result<usize, StrikeError> {
        let (body, _) = self
            .nearest_body(physics, point)
            .ok_or(StrikeError::BodyMissing)?;
        physics.apply_impulse_at(self.entities[body], impulse, point)?;
        Ok(body)
    }

    /// [`strike`](Self::strike) with what a recorded contact carried: its
    /// normal impulse, along its normal, at its point.
    ///
    /// The solver already applied that impulse to the body it struck — the
    /// living character's — so carrying it over to the ragdoll that replaced
    /// that body is applying it once. Transfer each contact once.
    ///
    /// # Errors
    ///
    /// [`StrikeError::AlreadyApplied`] for a contact that struck one of this
    /// ragdoll's own bodies, and [`strike`](Self::strike)'s errors.
    pub fn strike_with(
        &self,
        physics: &mut PhysicsSystem,
        contact: &KineticContact,
    ) -> Result<usize, StrikeError> {
        if self.entities.contains(&contact.struck) {
            return Err(StrikeError::AlreadyApplied);
        }
        self.strike(physics, contact.normal * contact.impulse, contact.point)
    }

    /// Writes the skeletal pose the bodies stand in into `pose`, placed by
    /// `model` — usually [`handoff_model`](Self::handoff_model). Joints with
    /// no body follow their parents. Compute the skinning palette from it as
    /// from any pose.
    ///
    /// # Errors
    ///
    /// [`RagdollError::BodyCount`] if a body is gone from the system, and the
    /// pose bridge's own errors; `pose` is unchanged on error.
    pub fn pose_into(
        &mut self,
        physics: &PhysicsSystem,
        model: Mat4,
        pose: &mut Pose,
    ) -> Result<(), RagdollError> {
        for (world, &entity) in self.worlds.iter_mut().zip(&self.entities) {
            *world = matrix(physics.transform(entity).ok_or(RagdollError::BodyCount)?);
        }
        self.binding.pose_into(model, &self.worlds, pose)
    }

    /// Whether every body sleeps: the ragdoll has come to rest.
    #[must_use]
    pub fn is_settled(&self, physics: &PhysicsSystem) -> bool {
        self.entities
            .iter()
            .all(|&entity| physics.is_sleeping(entity))
    }

    /// Puts every body to sleep where it stands: the force-settle a budget or
    /// a timeout asks for. Returns whether the ragdoll is settled afterwards,
    /// which is `false` in a system with sleep turned off.
    pub fn force_settle(&self, physics: &mut PhysicsSystem) -> bool {
        // Every body: a rig whose bodies are not all jointed together is
        // several islands, and a call on a body already asleep changes nothing.
        for &entity in &self.entities {
            physics.put_to_sleep(entity);
        }
        self.is_settled(physics)
    }

    /// Where each body's skeleton joint origin stands in the world, by joint,
    /// in rig order: what a server sends for [`pull_towards`](Self::pull_towards).
    /// Joint origins rather than body centres, because two rigs of one
    /// skeleton agree on where a joint is and not on the shape around it.
    /// `None` if a body is gone.
    #[must_use]
    pub fn joint_positions(&self, physics: &PhysicsSystem) -> Option<Vec<(usize, DVec3)>> {
        self.entities
            .iter()
            .zip(&self.skeleton_joints)
            .zip(&self.joint_in_body)
            .map(|((&entity, &joint), &local)| {
                let at = physics.transform(entity)?;
                Some((joint, at.position + at.rotation * local))
            })
            .collect()
    }

    /// Pulls each body whose skeleton joint has a target so that its joint
    /// origin moves towards it, as a critically damped spring of `hertz`: the
    /// plan's soft convergence of a client's detailed ragdoll on the server's
    /// simplified one, which keeps the two within a body's width before the
    /// settled pose arrives. `targets` are the server ragdoll's
    /// [`joint_positions`](Self::joint_positions), whose joints are some of
    /// this rig's. Call it once a tick before the step. Returns how many
    /// bodies were pulled.
    ///
    /// The spring is against the world, so a pulled body also loses speed as
    /// the server's comes to rest; a target for a joint this rig gives no body
    /// is ignored, and so is a non-finite one.
    pub fn pull_towards(
        &self,
        physics: &mut PhysicsSystem,
        targets: &[(usize, DVec3)],
        hertz: f64,
        dt: f64,
    ) -> usize {
        if !(hertz.is_finite() && hertz > 0.0 && dt.is_finite() && dt > 0.0) {
            return 0;
        }
        let omega = core::f64::consts::TAU * hertz;
        let mut pulled = 0;
        for &(joint, target) in targets {
            let Some(index) = self.skeleton_joints.iter().position(|&j| j == joint) else {
                continue;
            };
            let entity = self.entities[index];
            let Some(at) = physics
                .transform(entity)
                .map(|t| t.position + t.rotation * self.joint_in_body[index])
            else {
                continue;
            };
            if !target.is_finite() {
                continue;
            }
            if let Some(body) = physics.body_mut(entity) {
                // The spring-damper solved implicitly over the tick, so it is
                // stable at any frequency: the explicit step ran away once
                // the frequency neared the tick rate.
                let stiffness = omega * omega * dt;
                body.velocity = (body.velocity + stiffness * (target - at))
                    / (1.0 + 2.0 * omega * dt + stiffness * dt);
                pulled += 1;
            }
        }
        pulled
    }

    /// Puts every body's query collider on the layers in `bits` (see
    /// [`PhysicsSystem::set_collider_layers`]): whether a corpse blocks a
    /// character's movement, a camera's sweep or a revival's clearance check
    /// is the game's call, made by the masks those queries carry. Layers
    /// filter queries only; the bodies still collide with other bodies.
    pub fn set_query_layers(&self, physics: &mut PhysicsSystem, bits: u32) {
        for &entity in &self.entities {
            physics.set_collider_layers(entity, bits);
        }
    }

    /// Sets the solver substeps every body asks for — the plan's distance
    /// LOD: a ragdoll far from every camera steps coarser than one in view.
    /// Zero is the system's own count, below which no body goes (see
    /// [`PhysicsSystem::set_substeps`]); the rig's count is what it spawned
    /// with.
    ///
    /// A settled ragdoll is left alone, since changing a body's substeps
    /// wakes it and a sleeping one costs nothing to simulate. Returns whether
    /// the substeps were set.
    pub fn set_substeps(&self, physics: &mut PhysicsSystem, substeps: u32) -> bool {
        if self.is_settled(physics) {
            return false;
        }
        for &entity in &self.entities {
            physics.set_substeps(entity, substeps);
        }
        true
    }

    /// Every body's current transform, in rig order: the settled pose to
    /// keep or send. `None` if a body is gone.
    #[must_use]
    pub fn body_transforms(&self, physics: &PhysicsSystem) -> Option<Vec<Transform>> {
        self.entities
            .iter()
            .map(|&entity| physics.transform(entity).copied())
            .collect()
    }

    /// Takes every body, its joints and its ignored pairs out of `physics`.
    pub fn remove(self, physics: &mut PhysicsSystem) {
        for entity in self.entities {
            physics.remove_entity(entity);
        }
    }
}

/// A rigid transform as the animation side's matrix.
fn matrix(transform: &Transform) -> Mat4 {
    Mat4::from_rotation_translation(transform.rotation.as_quat(), transform.position.as_vec3())
}
