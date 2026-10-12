//! [`PhysicsSystem`]'s collision filter: which bodies the contact solver lets
//! touch, by category and mask, as Box2D's `b2Filter` does.
//!
//! Every body is in one or more categories and collides with the categories
//! its mask names; a pair collides only when each one's category is in the
//! other's mask. A game sorts its bodies once — characters, loose items,
//! corpses — instead of ignoring pairs one at a time, which is
//! [`PhysicsSystem::ignore_contacts`]' job for the odd pair no rule covers.
//!
//! This filters the solver only. Queries have their own filter, the layers of
//! [`PhysicsSystem::set_collider_layers`]: a corpse can be solid to the
//! floor, see-through to loose items and still struck by a bullet ray. Planes
//! are world geometry and collide with every body.

use crcbl_ecs::Entity;

use super::PhysicsSystem;

/// A body's categories and the categories it collides with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CollisionFilter {
    /// The categories the body is in, one bit each.
    pub category: u32,
    /// The categories it collides with.
    pub mask: u32,
}

impl CollisionFilter {
    /// The first category, colliding with every category: what every body
    /// starts with, so a system that sets no filter collides as before.
    pub const DEFAULT: Self = Self {
        category: 1,
        mask: u32::MAX,
    };

    /// Whether a body with this filter and one with `other` collide.
    #[must_use]
    pub const fn collides_with(self, other: Self) -> bool {
        self.category & other.mask != 0 && other.category & self.mask != 0
    }
}

impl Default for CollisionFilter {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl PhysicsSystem {
    /// Sets the categories `entity`'s body is in and those it collides with,
    /// and wakes it. Contacts the new filter forbids end at once; pairs it
    /// newly allows are found on the next step. Returns `false`, changing
    /// nothing, if the entity is not registered.
    pub fn set_collision_filter(&mut self, entity: Entity, filter: CollisionFilter) -> bool {
        let Some(&id) = self.entity_to_body.get(&entity) else {
            return false;
        };
        let Some(record) = self.records.get_mut(id) else {
            return false;
        };
        if record.filter == filter {
            return true;
        }
        record.filter = filter;
        self.wake(id);
        if let Some(pipeline) = self.contacts.as_mut() {
            pipeline.destroy_filtered_contacts_of(id, &self.records);
            if let Some(record) = self.records.get(id) {
                pipeline.rediscover(&record.proxies);
            }
        }
        true
    }

    /// `entity`'s collision filter, if it is registered.
    #[must_use]
    pub fn collision_filter(&self, entity: Entity) -> Option<CollisionFilter> {
        self.record(entity).map(|record| record.filter)
    }
}
