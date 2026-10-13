//! A client's detailed ragdoll held to the server's simplified one inside the
//! solver: **a spring joint from each shared joint origin to an anchor the
//! server's joint moves**.
//!
//! [`Ragdoll::pull_towards`] changes the bodies' velocities between steps, so
//! a step's contacts overrule it, and the two rigs part where they meet the
//! floor differently. A [`Tether`] puts the same critically damped spring in
//! the solver as a zero-length distance joint to a fixed anchor — an entity
//! with a transform and no body — so the spring and the contacts are solved
//! together, and [`Tether::follow`] moves the anchors to the server's
//! [`Ragdoll::joint_positions`] each tick before the step.
//!
//! The anchor stands still between moves, so the spring is against the world,
//! as the pull's is: a tethered body loses speed as the server's comes to
//! rest. Each anchor is the caller's entity, as the bodies are, and stays in
//! the system until [`Tether::remove`] takes it out with its joint.

use crcbl_ecs::Entity;
use crcbl_phys::{
    DistanceJoint, Joint, JointError, JointId, JointKind, PhysicsSystem, Spring, Transform,
};
use glam::{DQuat, DVec3};

use crate::instance::Ragdoll;

/// The spring joints holding a ragdoll's bodies to anchors the server's
/// joints move, made by [`Ragdoll::tether`].
#[derive(Debug)]
#[must_use = "a tether's anchors stay in the physics system until it is removed"]
pub struct Tether {
    links: Vec<Link>,
}

/// One tethered body: the skeleton joint whose server position its anchor
/// follows, the anchor, and the spring between them.
#[derive(Clone, Copy, Debug)]
struct Link {
    joint: usize,
    anchor: Entity,
    spring: JointId,
}

/// Why a ragdoll was not tethered.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TetherError {
    /// The spring's frequency is not finite and positive.
    #[error("a tether's spring of {0} Hz is not finite and positive")]
    Hertz(f64),
    /// One anchor per tethered body is needed.
    #[error("{anchors} anchors given for {needed} tethered bodies")]
    AnchorCount {
        /// How many were given.
        anchors: usize,
        /// How many bodies have a target.
        needed: usize,
    },
    /// An anchor is already in the physics system.
    #[error("anchor {0:?} is already in the physics system")]
    AnchorInUse(Entity),
    /// The physics system refused a spring.
    #[error(transparent)]
    Joint(#[from] JointError),
}

impl Ragdoll {
    /// Tethers each body whose skeleton joint has a finite target in
    /// `targets` — the server ragdoll's [`joint_positions`](Self::joint_positions),
    /// whose joints are some of this rig's — to an anchor placed at it, by a
    /// critically damped spring of `hertz` from the body's joint origin. The
    /// anchors are taken from `anchors` in the order the targets come, one
    /// per tethered body, none of them yet in `physics`.
    ///
    /// Measured on the synthetic humanoid's detailed and coarse rigs, struck
    /// alike and landing differently: tethered at 45 Hz the shared joints
    /// peak 0.14 m apart through the landing, against 0.26 m for
    /// [`pull_towards`](Self::pull_towards) at 60 Hz, the stiffest pull
    /// tried. A stiffer tether did worse: at 60 Hz the joints peaked 0.29 m
    /// apart.
    ///
    /// # Errors
    ///
    /// [`TetherError`] for a frequency out of range, too few anchors, an
    /// anchor already in `physics`, or a spring the system refuses — which
    /// takes the anchors placed so far back out.
    ///
    /// # Panics
    ///
    /// Panics if `physics` has no contacts: joints are contact-solver
    /// constraints.
    pub fn tether(
        &self,
        physics: &mut PhysicsSystem,
        targets: &[(usize, DVec3)],
        anchors: &[Entity],
        hertz: f64,
    ) -> Result<Tether, TetherError> {
        if !(hertz.is_finite() && hertz > 0.0) {
            return Err(TetherError::Hertz(hertz));
        }
        let tethered: Vec<(usize, usize, DVec3)> = targets
            .iter()
            .filter(|(_, target)| target.is_finite())
            .filter_map(|&(joint, target)| {
                let body = self.skeleton_joints.iter().position(|&j| j == joint)?;
                Some((body, joint, target))
            })
            .collect();
        if anchors.len() < tethered.len() {
            return Err(TetherError::AnchorCount {
                anchors: anchors.len(),
                needed: tethered.len(),
            });
        }
        if let Some(&taken) = anchors
            .iter()
            .take(tethered.len())
            .find(|&&anchor| physics.transform(anchor).is_some())
        {
            return Err(TetherError::AnchorInUse(taken));
        }
        let mut links = Vec::with_capacity(tethered.len());
        for (&(body, joint, target), &anchor) in tethered.iter().zip(anchors) {
            physics.set_transform(anchor, Transform::new(target, DQuat::IDENTITY));
            let spring = JointKind::Distance(DistanceJoint {
                enable_spring: true,
                spring: Spring {
                    hertz,
                    damping_ratio: 1.0,
                },
                ..DistanceJoint::rigid(0.0)
            });
            let held = Joint::new(
                self.entities[body],
                anchor,
                Transform::new(self.joint_in_body[body], DQuat::IDENTITY),
                Transform::IDENTITY,
                spring,
            );
            match physics.add_joint(held) {
                Ok(spring) => links.push(Link {
                    joint,
                    anchor,
                    spring,
                }),
                Err(error) => {
                    physics.remove_entity(anchor);
                    Tether { links }.remove(physics);
                    return Err(error.into());
                }
            }
        }
        Ok(Tether { links })
    }
}

impl Tether {
    /// Moves each anchor whose joint has a finite target in `targets` — the
    /// server's [`Ragdoll::joint_positions`] this tick — to it. Call it once a
    /// tick before the step. Returns how many anchors moved.
    pub fn follow(&self, physics: &mut PhysicsSystem, targets: &[(usize, DVec3)]) -> usize {
        let mut moved = 0;
        for link in &self.links {
            let target = targets
                .iter()
                .find(|(joint, target)| *joint == link.joint && target.is_finite());
            if let Some(&(_, target)) = target {
                physics.set_transform(link.anchor, Transform::new(target, DQuat::IDENTITY));
                moved += 1;
            }
        }
        moved
    }

    /// How many bodies are tethered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// Whether no body is tethered: none of the targets was this rig's.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// Takes every anchor and its spring out of `physics`, letting the
    /// bodies go. A spring a removed ragdoll already took with its body is
    /// not looked for.
    pub fn remove(self, physics: &mut PhysicsSystem) {
        for link in self.links {
            physics.remove_joint(link.spring);
            physics.remove_entity(link.anchor);
        }
    }
}
