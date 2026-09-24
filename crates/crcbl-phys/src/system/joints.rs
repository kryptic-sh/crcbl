//! [`PhysicsSystem`]'s joints and solver groups: rung 5 of
//! the contact solver (`docs/notes/simulation.md`). See [`crate::joint`] for
//! the joints and `crate::contact::group` for the groups.

use crcbl_ecs::Entity;
use glam::DVec3;

use super::{BodyId, PhysicsSystem};
use crate::contact::joint;
use crate::joint::{Joint, JointBreak, JointDrift, JointError, JointId};

impl PhysicsSystem {
    /// Adds `joint` and wakes both its bodies — decision 4's "a new joint"
    /// wake rule. Unless it lets them collide, every contact between them is
    /// ended.
    ///
    /// # Errors
    ///
    /// [`JointError::UnknownBody`] if either entity is not registered — a
    /// fixed anchor is an entity given a transform with
    /// [`set_transform`](Self::set_transform) — [`JointError::SameBody`] if
    /// the two are one, and [`JointError::Invalid`] for a setting out of
    /// range.
    ///
    /// # Panics
    ///
    /// Panics if this system has no contacts: joints are constraints in the
    /// contact solver. Build it [`with_contacts`](Self::with_contacts).
    pub fn add_joint(&mut self, joint: Joint) -> Result<JointId, JointError> {
        assert!(
            self.contacts.is_some(),
            "joints are contact-solver constraints: build the system with_contacts"
        );
        let (a, b) = self.joint_bodies(&joint)?;
        self.wake(a);
        self.wake(b);
        let pipeline = self.contacts.as_mut().expect("asserted above");
        if !joint.collide_connected {
            pipeline.destroy_contacts_between(a, b);
        }
        Ok(pipeline.joints.insert(joint, a, b))
    }

    /// Takes joint `id` out, waking both its bodies, and returns it; `None`
    /// if it is not there — never added, removed, or broken.
    pub fn remove_joint(&mut self, id: JointId) -> Option<Joint> {
        self.take_out_joint(id)
    }

    /// Joint `id`, if it is there.
    #[must_use]
    pub fn joint(&self, id: JointId) -> Option<&Joint> {
        self.contacts
            .as_ref()?
            .joints
            .get(id)
            .map(|record| &record.joint)
    }

    /// Replaces joint `id` with `joint` — a new motor speed, new limits,
    /// other bodies — waking its bodies old and new. What it carries is kept
    /// for the warm start while it stays the same kind between the same
    /// bodies. Returns `Ok(false)` if `id` names no joint.
    ///
    /// # Errors
    ///
    /// As [`add_joint`](Self::add_joint)'s, and the joint is left as it was.
    pub fn set_joint(&mut self, id: JointId, joint: Joint) -> Result<bool, JointError> {
        let Some(old) = self.joint(id).copied() else {
            return Ok(false);
        };
        let (a, b) = self.joint_bodies(&joint)?;
        let (old_a, old_b) = self.joint_bodies(&old)?;
        for body in [a, b, old_a, old_b] {
            self.wake(body);
        }
        if old.collide_connected != joint.collide_connected || (old_a, old_b) != (a, b) {
            self.let_collide(old_a, old_b);
        }
        let pipeline = self.contacts.as_mut().expect("a joint lives in one");
        if !joint.collide_connected {
            pipeline.destroy_contacts_between(a, b);
        }
        Ok(pipeline.joints.replace(id, joint, a, b))
    }

    /// Every joint and its id, in the order they were added into the pool's
    /// slots.
    pub fn joints(&self) -> impl Iterator<Item = (JointId, &Joint)> {
        self.contacts
            .iter()
            .flat_map(|pipeline| pipeline.joints.iter())
            .map(|(id, record)| (id, &record.joint))
    }

    /// How many joints there are.
    #[must_use]
    pub fn joint_count(&self) -> usize {
        self.contacts
            .as_ref()
            .map_or(0, |pipeline| pipeline.joints.len())
    }

    /// The force, in newtons, and torque, in newton-metres, joint `id`
    /// carried over the last substep it was solved in: what its thresholds
    /// are held to.
    #[must_use]
    pub fn joint_reaction(&self, id: JointId) -> Option<(f64, f64)> {
        self.contacts.as_ref()?.joints.reaction(id)
    }

    /// Where joint `id`'s frames are in the world now: its anchor on body A
    /// and its anchor on body B, which it holds together or apart.
    #[must_use]
    pub fn joint_anchors(&self, id: JointId) -> Option<(DVec3, DVec3)> {
        let joint = self.joint(id)?;
        let at = |entity: Entity, local: DVec3| {
            let transform = self.transform(entity)?;
            Some(transform.position + transform.rotation * local)
        };
        Some((
            at(joint.body_a, joint.frame_a.position)?,
            at(joint.body_b, joint.frame_b.position)?,
        ))
    }

    /// How far joint `id` is from holding, with its bodies where they are
    /// now: see [`JointDrift`].
    #[must_use]
    pub fn joint_drift(&self, id: JointId) -> Option<JointDrift> {
        let joint = self.joint(id)?;
        Some(joint::drift(
            joint,
            self.transform(joint.body_a)?,
            self.transform(joint.body_b)?,
        ))
    }

    /// The joints the last [`step`](Self::step) broke, and took out.
    #[must_use]
    pub fn broken_joints(&self) -> &[JointBreak] {
        self.contacts
            .as_ref()
            .map_or(&[], |pipeline| pipeline.joints.broken.as_slice())
    }

    /// Asks the solver to give `entity`'s body `substeps` substeps a tick,
    /// and every body it is joined to by a contact or a joint the same:
    /// decision 1's extra substeps per group. A group runs the most any of
    /// its bodies asked for, and never fewer than
    /// [`crate::ContactSettings::substeps`], so zero is the system's own;
    /// its constraints are stiffer in proportion — see the solver's groups
    /// in `crates/crcbl-phys/src/contact/group.rs`. Returns `false` if the
    /// entity is not registered.
    ///
    /// A bridge's planks or a tall column ask for more, and the rest of the
    /// system pays nothing for it.
    pub fn set_substeps(&mut self, entity: Entity, substeps: u32) -> bool {
        let Some(&id) = self.entity_to_body.get(&entity) else {
            return false;
        };
        self.wake(id);
        match self.records.get_mut(id) {
            Some(record) => {
                record.substeps = substeps;
                true
            }
            None => false,
        }
    }

    /// The substeps `entity` asked for with
    /// [`set_substeps`](Self::set_substeps), or `None` if it is not
    /// registered.
    #[must_use]
    pub fn substeps(&self, entity: Entity) -> Option<u32> {
        self.record(entity).map(|record| record.substeps)
    }

    /// Takes joint `id` out: wakes its bodies, tells their island it may be
    /// in pieces, and lets them collide again if it kept them apart.
    pub(crate) fn take_out_joint(&mut self, id: JointId) -> Option<Joint> {
        let record = self.contacts.as_mut()?.joints.remove(id)?;
        for body in [record.a, record.b] {
            self.wake(body);
            if let Some(island) = self.records.get(body).and_then(|r| r.island) {
                self.islands.mark_removed(island);
            }
        }
        if !record.joint.collide_connected {
            self.let_collide(record.a, record.b);
        }
        Some(record.joint)
    }

    /// Takes out every joint of body `id`: what removing its entity does.
    pub(super) fn take_out_joints_of(&mut self, id: BodyId) {
        let Some(pipeline) = self.contacts.as_ref() else {
            return;
        };
        for joint in pipeline.joints.of_body(id) {
            self.take_out_joint(joint);
        }
    }

    /// Has the broadphase look again for pairs between `a` and `b`, which a
    /// joint no longer keeps apart.
    fn let_collide(&mut self, a: BodyId, b: BodyId) {
        let Some(pipeline) = self.contacts.as_mut() else {
            return;
        };
        for body in [a, b] {
            if let Some(record) = self.records.get(body) {
                pipeline.rediscover(&record.proxies);
            }
        }
    }

    /// Both bodies of `joint`, if it can be added.
    fn joint_bodies(&self, joint: &Joint) -> Result<(BodyId, BodyId), JointError> {
        if let Some(why) = joint.invalid() {
            return Err(JointError::Invalid(why));
        }
        let id = |entity: Entity| {
            self.entity_to_body
                .get(&entity)
                .copied()
                .ok_or(JointError::UnknownBody(entity))
        };
        let (a, b) = (id(joint.body_a)?, id(joint.body_b)?);
        if a == b {
            return Err(JointError::SameBody);
        }
        Ok((a, b))
    }
}
