//! [`PhysicsSystem`]'s ignored pairs: two bodies that never collide with each
//! other, though no joint joins them. See `crate::contact::ignored`.

use crcbl_ecs::Entity;

use super::{BodyId, PhysicsSystem};

/// Why [`PhysicsSystem::ignore_contacts`] or
/// [`PhysicsSystem::restore_contacts`] refused a pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactPairError {
    /// An entity that is not registered with the system.
    UnknownBody(Entity),
    /// Both sides are one body.
    SameBody,
}

impl core::fmt::Display for ContactPairError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownBody(entity) => write!(f, "{entity:?} is not registered"),
            Self::SameBody => f.write_str("a contact pair is two different bodies"),
        }
    }
}

impl std::error::Error for ContactPairError {}

impl PhysicsSystem {
    /// Stops the bodies of `a` and `b` colliding with each other, and wakes
    /// both: every contact between them is ended, as a joint that keeps its
    /// bodies apart ends theirs. Each still collides with everything else.
    /// Returns `Ok(false)` if the pair was already ignored.
    ///
    /// This is for bodies that overlap by design without a joint between
    /// them, such as two limbs of one ragdoll meeting at its pelvis. The pair
    /// is forgotten when either entity is removed.
    ///
    /// # Errors
    ///
    /// [`ContactPairError::UnknownBody`] if either entity is not registered,
    /// and [`ContactPairError::SameBody`] if the two are one.
    ///
    /// # Panics
    ///
    /// Panics if this system has no contacts. Build it
    /// [`with_contacts`](Self::with_contacts).
    pub fn ignore_contacts(&mut self, a: Entity, b: Entity) -> Result<bool, ContactPairError> {
        let (a, b) = self.contact_pair(a, b)?;
        self.wake(a);
        self.wake(b);
        let pipeline = self.contacts.as_mut().expect("checked by contact_pair");
        let added = pipeline.ignored.insert(a, b);
        if added {
            pipeline.destroy_contacts_between(a, b);
        }
        Ok(added)
    }

    /// Lets the bodies of `a` and `b` collide with each other again, unless a
    /// joint still keeps them apart, and wakes both. Returns `Ok(false)` if
    /// the pair was not ignored.
    ///
    /// # Errors
    ///
    /// As [`ignore_contacts`](Self::ignore_contacts)'s.
    ///
    /// # Panics
    ///
    /// Panics if this system has no contacts.
    pub fn restore_contacts(&mut self, a: Entity, b: Entity) -> Result<bool, ContactPairError> {
        let (a, b) = self.contact_pair(a, b)?;
        self.wake(a);
        self.wake(b);
        let removed = self
            .contacts
            .as_mut()
            .expect("checked by contact_pair")
            .ignored
            .remove(a, b);
        if removed {
            self.let_collide(a, b);
        }
        Ok(removed)
    }

    /// Whether [`ignore_contacts`](Self::ignore_contacts) keeps the bodies of
    /// `a` and `b` apart. A joint's own rule is not counted here.
    #[must_use]
    pub fn ignores_contacts(&self, a: Entity, b: Entity) -> bool {
        let (Some(pipeline), Some(&a), Some(&b)) = (
            self.contacts.as_ref(),
            self.entity_to_body.get(&a),
            self.entity_to_body.get(&b),
        ) else {
            return false;
        };
        pipeline.ignored.contains(a, b)
    }

    /// Both bodies of a pair, if it can be ignored.
    fn contact_pair(&self, a: Entity, b: Entity) -> Result<(BodyId, BodyId), ContactPairError> {
        assert!(
            self.contacts.is_some(),
            "ignored pairs are a contact-pipeline rule: build the system with_contacts"
        );
        let id = |entity: Entity| {
            self.entity_to_body
                .get(&entity)
                .copied()
                .ok_or(ContactPairError::UnknownBody(entity))
        };
        let (a, b) = (id(a)?, id(b)?);
        if a == b {
            return Err(ContactPairError::SameBody);
        }
        Ok((a, b))
    }
}
