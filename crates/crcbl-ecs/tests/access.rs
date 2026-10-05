//! Declared access end to end, through the public API: the conflicts a
//! schedule derives from its systems' declarations, the declarations it
//! refuses, and — in debug builds — the panic on a tick touching a resource
//! its system did not declare.

#[cfg(debug_assertions)]
use std::panic::{AssertUnwindSafe, catch_unwind};

use crcbl_ecs::{
    Access, AccessError, Conflict, ConflictKind, DebugCtx, Entity, Schedule, Shared, SystemTrait,
    World,
};

/// A system that declares what it was built with and, each tick, runs what it
/// was built with — so a test can make it touch more than it declared.
struct Probe {
    name: &'static str,
    access: Access,
    on_tick: Box<dyn FnMut() + Send>,
}

impl Probe {
    fn new(
        name: &'static str,
        access: Access,
        on_tick: impl FnMut() + Send + 'static,
    ) -> Box<Self> {
        Box::new(Self {
            name,
            access,
            on_tick: Box::new(on_tick),
        })
    }

    /// Declares `access` and touches nothing.
    fn idle(name: &'static str, access: Access) -> Box<Self> {
        Self::new(name, access, || {})
    }
}

impl SystemTrait for Probe {
    fn name(&self) -> &str {
        self.name
    }
    fn access(&self) -> Access {
        self.access.clone()
    }
    fn tick(&mut self, _dt: f64) {
        (self.on_tick)();
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

/// A schedule with each of `names` registered as a resource holding `0`.
fn schedule_sharing(names: &[&str]) -> Schedule {
    let mut schedule = Schedule::new();
    for &name in names {
        schedule
            .share(&Shared::new(name, 0_u32))
            .expect("distinct names");
    }
    schedule
}

fn conflict(before: usize, after: usize, resource: &str, kind: ConflictKind) -> Conflict {
    Conflict {
        before,
        after,
        resource: resource.to_owned(),
        kind,
    }
}

/// Two writers conflict, a reader and a writer conflict either way round, and
/// two readers do not: the two readers here have no edge between them, and
/// every edge points from the earlier registration to the later.
#[test]
fn writes_conflict_with_writes_and_reads_but_reads_do_not_conflict_with_reads() {
    let mut schedule = schedule_sharing(&["wind"]);
    schedule.add_system(Probe::idle("gust", Access::none().writes("wind")));
    schedule.add_system(Probe::idle("sail", Access::none().reads("wind")));
    schedule.add_system(Probe::idle("kite", Access::none().reads("wind")));
    schedule.add_system(Probe::idle("storm", Access::none().writes("wind")));

    assert_eq!(
        schedule.conflicts(),
        [
            conflict(0, 1, "wind", ConflictKind::ReadWrite),
            conflict(0, 2, "wind", ConflictKind::ReadWrite),
            conflict(0, 3, "wind", ConflictKind::WriteWrite),
            conflict(1, 3, "wind", ConflictKind::ReadWrite),
            conflict(2, 3, "wind", ConflictKind::ReadWrite),
        ]
    );
}

#[test]
fn systems_declaring_nothing_shared_have_no_conflicts() {
    let mut schedule = Schedule::new();
    schedule.add_system(Probe::idle("a", Access::none()));
    schedule.add_system(Probe::idle("b", Access::none()));
    assert_eq!(schedule.conflicts(), []);
}

/// The same registrations give the same graph, whatever order each system
/// listed its names in: the edges come out by later system, earlier system,
/// then resource name. The first system reads a name that sorts after the
/// ones it writes, so a graph walking its reads and then its writes would list
/// `wind` first.
#[test]
fn the_conflict_graph_does_not_depend_on_the_order_names_were_declared_in() {
    let build = |gust: Access, storm: Access| {
        let mut schedule = schedule_sharing(&["wind", "rain", "score"]);
        schedule.add_system(Probe::idle("gust", gust));
        schedule.add_system(Probe::idle("storm", storm));
        schedule.conflicts().to_vec()
    };
    let first = build(
        Access::none().reads("wind").writes("rain").writes("score"),
        Access::none().writes("wind").reads("rain").writes("score"),
    );
    let second = build(
        Access::none().writes("score").writes("rain").reads("wind"),
        Access::none().writes("score").reads("rain").writes("wind"),
    );
    let expected = [
        conflict(0, 1, "rain", ConflictKind::ReadWrite),
        conflict(0, 1, "score", ConflictKind::WriteWrite),
        conflict(0, 1, "wind", ConflictKind::ReadWrite),
    ];
    assert_eq!(first, expected);
    assert_eq!(second, expected);
}

/// The refusal names the system and the resource, and the schedule is left
/// without the system rather than holding one whose conflicts it never
/// worked out.
#[test]
fn a_declaration_naming_an_unregistered_resource_is_refused_by_name() {
    let mut schedule = schedule_sharing(&["wind"]);
    let refused = schedule.try_add_system(Probe::idle(
        "mover",
        Access::none().reads("wind").writes("tide"),
    ));
    assert_eq!(
        refused,
        Err(AccessError::UnknownResource {
            system: "mover".to_owned(),
            resource: "tide".to_owned(),
        })
    );
    assert!(schedule.is_empty());

    assert_eq!(
        schedule.try_add_system(Probe::idle("mover", Access::none().reads("wind"))),
        Ok(()),
        "the registered name alone is accepted"
    );
}

#[test]
#[should_panic(
    expected = "system `mover` declares shared resource `tide`, which is not registered"
)]
fn registering_a_system_that_names_an_unregistered_resource_panics_naming_both() {
    World::new().register_system(Probe::idle("mover", Access::none().writes("tide")));
}

#[test]
fn a_second_resource_of_one_name_is_refused() {
    let mut world = World::new();
    world.share(&Shared::new("wind", 1)).expect("the first");
    assert_eq!(
        world.share(&Shared::new("wind", 2)),
        Err(AccessError::DuplicateResource {
            resource: "wind".to_owned(),
        })
    );
}

/// A world sharing `wind`, holding `0`, and one system, `sneak`, declaring
/// `access` and running on each tick what `on_tick` builds from its own handle
/// to `wind`.
fn world_with<F: FnMut() + Send + 'static>(
    access: Access,
    on_tick: impl FnOnce(Shared<u32>) -> F,
) -> (World, Shared<u32>) {
    let wind = Shared::new("wind", 0);
    let mut world = World::new();
    world.share(&wind).expect("the world's only resource");
    world.register_system(Probe::new("sneak", access, on_tick(wind.clone())));
    (world, wind)
}

/// Declared accesses go through, and the tick really did reach the value: a
/// check that passed because the tick never ran would pass this too, without
/// the count.
#[test]
fn a_tick_touching_what_it_declared_runs() {
    let wind = Shared::new("wind", 0_u32);
    let gauge = Shared::new("gauge", 0_u32);
    let mut world = World::new();
    world.share(&wind).expect("distinct");
    world.share(&gauge).expect("distinct");
    let (read, written) = (wind.clone(), gauge.clone());
    world.register_system(Probe::new(
        "anemometer",
        Access::none().reads("wind").writes("gauge"),
        move || *written.write() += *read.read() + 1,
    ));
    *wind.write() = 4;

    world.tick();
    world.tick();
    assert_eq!(*gauge.read(), 10);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(
    expected = "system `sneak` read shared resource `wind` without declaring that access"
)]
fn a_tick_reading_what_it_did_not_declare_panics_naming_system_and_resource() {
    let (mut world, _) = world_with(Access::none(), |wind| {
        move || {
            let _ = *wind.read();
        }
    });
    world.tick();
}

/// A read declaration does not cover a write.
#[cfg(debug_assertions)]
#[test]
#[should_panic(
    expected = "system `sneak` wrote shared resource `wind` without declaring that access"
)]
fn a_tick_writing_what_it_declared_only_as_read_panics() {
    let (mut world, _) = world_with(Access::none().reads("wind"), |wind| {
        move || *wind.write() = 1
    });
    world.tick();
}

/// Once a tick has panicked and the panic is caught, nothing is left
/// running: the code that caught it touches the resource freely, as code
/// outside a tick always may.
#[cfg(debug_assertions)]
#[test]
fn a_tick_that_panics_leaves_no_system_running_behind_it() {
    let (mut world, wind) = world_with(Access::none(), |wind| move || *wind.write() = 1);
    let payload =
        catch_unwind(AssertUnwindSafe(|| world.tick())).expect_err("the undeclared write panics");
    let message = payload
        .downcast_ref::<String>()
        .expect("the panic carries a formatted message");
    assert!(
        message.contains("`sneak`") && message.contains("`wind`"),
        "{message}"
    );

    *wind.write() = 2;
    assert_eq!(*wind.read(), 2, "the write outside the tick went through");
}

/// Code outside a tick — set-up, a game module — holds the world exclusively
/// and is never checked, whatever the registered systems declared.
#[test]
fn outside_a_tick_nothing_is_checked() {
    let (mut world, wind) = world_with(Access::none(), |_| || {});
    world.tick();
    *wind.write() += 3;
    assert_eq!(*wind.read(), 3);
}
