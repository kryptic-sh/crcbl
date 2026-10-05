use crate::tick_time::TickTime;
use crate::world::World;

/// Per-system stats collected by [`Inspector::collect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStats {
    /// The system's name (from [`SystemTrait::name`](crate::SystemTrait::name)).
    pub name: String,
    /// Number of entities currently registered with the system.
    pub entity_count: usize,
    /// How long the system's ticks took, or `None` when its schedule has no
    /// clock ([`Schedule::set_clock`](crate::Schedule::set_clock)) or it has
    /// not ticked since the clock was set.
    pub tick_time: Option<TickTime>,
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
        let schedule = world.schedule();
        schedule
            .stats()
            .zip(schedule.tick_times())
            .map(|((name, entity_count), tick_time)| SystemStats {
                name,
                entity_count,
                tick_time,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use crcbl_core::time::TimeSource;

    use super::*;
    use crate::tick_time::TICK_TIME_WINDOW;
    use crate::{DebugCtx, Entity, System, SystemTrait};

    /// A clock the test's systems move: reading it costs nothing, and a tick
    /// takes exactly as long as the system says it did.
    #[derive(Debug, Clone, Default)]
    struct FakeClock(Rc<Cell<Duration>>);

    impl TimeSource for FakeClock {
        fn elapsed(&self) -> Duration {
            self.0.get()
        }
    }

    /// A system whose `n`th tick advances the clock by `cost(n)`, counting
    /// from 1.
    struct Costly {
        name: &'static str,
        clock: FakeClock,
        ticks: u32,
        cost: fn(u32) -> Duration,
    }

    impl Costly {
        fn new(name: &'static str, clock: &FakeClock, cost: fn(u32) -> Duration) -> Self {
            Self {
                name,
                clock: clock.clone(),
                ticks: 0,
                cost,
            }
        }
    }

    impl SystemTrait for Costly {
        fn name(&self) -> &str {
            self.name
        }
        fn tick(&mut self, _dt: f64) {
            self.ticks += 1;
            let now = self.clock.0.get();
            self.clock.0.set(now + (self.cost)(self.ticks));
        }
        fn entity_count(&self) -> usize {
            0
        }
        fn sweep(&mut self, _dead: &[Entity]) {}
        fn debug_draw(&mut self, _ctx: &DebugCtx) {}
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn tick_times(world: &World) -> Vec<Option<TickTime>> {
        Inspector::collect(world)
            .into_iter()
            .map(|stats| stats.tick_time)
            .collect()
    }

    #[test]
    fn each_systems_tick_time_is_what_the_clock_says_it_took() {
        let clock = FakeClock::default();
        let mut world = World::new();
        world.register_system(Box::new(Costly::new("a", &clock, |_| {
            Duration::from_millis(3)
        })));
        world.register_system(Box::new(Costly::new("free", &clock, |_| Duration::ZERO)));
        world.register_system(Box::new(Costly::new("b", &clock, |_| {
            Duration::from_millis(5)
        })));
        world
            .schedule_mut()
            .set_clock(Some(Box::new(clock.clone())));
        // Time passing between ticks is nobody's tick.
        clock.0.set(Duration::from_secs(7));

        world.tick();
        let took = |ms| {
            Some(TickTime {
                last: Duration::from_millis(ms),
                mean: Duration::from_millis(ms),
            })
        };
        assert_eq!(tick_times(&world), [took(3), took(0), took(5)]);
    }

    #[test]
    fn the_mean_covers_every_tick_until_the_window_fills_and_the_window_after() {
        let clock = FakeClock::default();
        let mut world = World::new();
        world.register_system(Box::new(Costly::new("ramp", &clock, |n| {
            Duration::from_millis(u64::from(n))
        })));
        world.schedule_mut().set_clock(Some(Box::new(clock)));

        for _ in 0..3 {
            world.tick();
        }
        assert_eq!(
            tick_times(&world),
            [Some(TickTime {
                last: Duration::from_millis(3),
                mean: Duration::from_millis(2),
            })],
            "three ticks of 1, 2 and 3 ms"
        );

        let past = 10;
        let ticks = TICK_TIME_WINDOW as u64 + past;
        for _ in 3..ticks {
            world.tick();
        }
        // Only the newest window's ticks count: past + 1 through `ticks` ms.
        let window: u64 = (past + 1..=ticks).sum();
        assert_eq!(
            tick_times(&world),
            [Some(TickTime {
                last: Duration::from_millis(ticks),
                mean: Duration::from_millis(window) / TICK_TIME_WINDOW as u32,
            })]
        );
    }

    #[test]
    fn a_schedule_without_a_clock_reports_no_tick_time() {
        let clock = FakeClock::default();
        let mut world = World::new();
        world.register_system(Box::new(Costly::new("a", &clock, |_| {
            Duration::from_millis(2)
        })));
        world.tick();
        assert_eq!(tick_times(&world), [None], "never given a clock");
        assert!(!world.schedule().is_timed());

        world
            .schedule_mut()
            .set_clock(Some(Box::new(clock.clone())));
        assert_eq!(tick_times(&world), [None], "timed, but not ticked yet");
        world.tick();
        assert!(tick_times(&world)[0].is_some());

        world.schedule_mut().set_clock(Some(Box::new(clock)));
        assert_eq!(
            tick_times(&world),
            [None],
            "a new clock drops the old one's times"
        );
        world.tick();
        world.schedule_mut().set_clock(None);
        world.tick();
        assert_eq!(tick_times(&world), [None], "and no clock drops them too");
    }

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
                tick_time: None,
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
                tick_time: None,
            }
        );
        assert_eq!(
            stats[1],
            SystemStats {
                name: "b".into(),
                entity_count: 1,
                tick_time: None,
            }
        );
    }
}
