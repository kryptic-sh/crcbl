use crate::world::World;

/// Per-system stats collected by [`Inspector::collect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStats {
    /// The system's name (from [`SystemTrait::name`](crate::SystemTrait::name)).
    pub name: String,
    /// Number of entities currently registered with the system.
    pub entity_count: usize,
}

/// Collects system statistics from a [`World`].
///
/// ```rust
/// # use crcbl_ecs::*;
/// let mut world = World::new();
/// let mut sys = System::<i32>::new("physics");
/// sys.attach(world.spawn(), 1);
/// world.register_system(Box::new(sys));
///
/// let stats = Inspector::collect(&world);
/// assert_eq!(stats.len(), 1);
/// assert_eq!(stats[0].name, "physics");
/// assert_eq!(stats[0].entity_count, 1);
/// ```
#[derive(Debug)]
pub struct Inspector;

impl Inspector {
    /// Returns one [`SystemStats`] per system currently registered in
    /// `world`'s schedule.
    #[must_use]
    pub fn collect(world: &World) -> Vec<SystemStats> {
        world
            .schedule()
            .stats()
            .map(|(name, entity_count)| SystemStats { name, entity_count })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::System;

    #[test]
    fn an_inspector_over_a_world_with_no_systems_collects_nothing() {
        let world = World::new();
        let stats = Inspector::collect(&world);
        assert!(stats.is_empty());
    }

    #[test]
    fn a_single_registered_system_is_reported_with_its_name_and_entity_count() {
        let mut world = World::new();
        let e = world.spawn();
        let mut sys = System::<i32>::new("movement");
        sys.attach(e, 42);
        world.register_system(Box::new(sys));

        let stats = Inspector::collect(&world);
        assert_eq!(
            stats,
            vec![SystemStats {
                name: "movement".into(),
                entity_count: 1,
            }]
        );
    }

    #[test]
    fn several_systems_are_reported_in_registration_order_each_with_its_own_count() {
        let mut world = World::new();
        let e1 = world.spawn();
        let e2 = world.spawn();

        let mut sys_a = System::<i32>::new("a");
        sys_a.attach(e1, 1);
        sys_a.attach(e2, 2);

        let mut sys_b = System::<f64>::new("b");
        sys_b.attach(e1, 1.0);

        world.register_system(Box::new(sys_a));
        world.register_system(Box::new(sys_b));

        let stats = Inspector::collect(&world);
        assert_eq!(stats.len(), 2);
        assert_eq!(
            stats[0],
            SystemStats {
                name: "a".into(),
                entity_count: 2,
            }
        );
        assert_eq!(
            stats[1],
            SystemStats {
                name: "b".into(),
                entity_count: 1,
            }
        );
    }
}
