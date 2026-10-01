//! The stage, played in a tool's world: the module `apps/editor`'s play mode
//! runs on this game's field.
//!
//! ```text
//!   the scene's files ──▶ Map::load ──▶ Stage ──▶ TowersModule::tick, as the
//!                                         │         one local player
//!                                         └──▶ every creep, as a Walker entity
//! ```
//!
//! # The game's own tick, as one local player asking for nothing
//!
//! `run_team_tick` holds the run still on a tick with no command frame at all —
//! a dedicated server nobody has joined — and a tool has no client to send
//! one. So this module ticks through [`TowersModule`]'s [`GameModule`] half,
//! which is solo's: it reads the [`ClientInputs`] it is handed as the frames of
//! the one local player, and an empty set is that player asking for nothing
//! this tick — `run_tick` hands `run_team_tick` one empty command, not none.
//! The build phase runs down, the waves come, the creeps walk and leak, a lost
//! run starts itself again: the run solo plays with nobody at the keys, by the
//! same rules, because it is the same call.
//!
//! # Creeps as entities a tool can draw
//!
//! The stage keeps its creeps outside the ECS — see `crate::game`'s module
//! docs — and a tool draws entities. So after every tick this module mirrors
//! each creep as a [`Walker`] in the [`WALKERS`] system, keyed by the creep's
//! physics body so one creep is one entity for as long as it lives, and
//! despawns the entity of a creep that died or leaked. `Walker` is a
//! **runtime** component ([`crcbl::registry::Registry::register_runtime`]): a
//! tool draws it from its placement and never lists, edits or saves it.
//!
//! Towers, bolts and bursts are not mirrored: no tool can build a tower yet,
//! so a field it plays has none of the three.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crcbl::assets::AssetSource;
use crcbl::ecs::{ClientInputs, ComponentHash, Entity, GameModule, System, World};
use crcbl::math::DVec3;
use crcbl::phys::ColliderId;
use crcbl::registry::{OrientedBox, Placement, Registry};

use super::{DEFAULT_TICK_HZ, Stage, TowersModule, lock};
use crate::map::{CREEP_RADIUS, Map};

/// The runtime system every [`Walker`] is a row of, while the field plays.
pub(crate) const WALKERS: &str = "walkers";

/// One creep on a played field, as a tool draws it: where its centre is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Walker {
    centre: DVec3,
}

impl ComponentHash for Walker {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        for value in self.centre.to_array() {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// The box around the creep's sphere, which is what a greybox draws it as.
impl Placement for Walker {
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::axis_aligned(
            self.centre,
            DVec3::splat(CREEP_RADIUS),
        ))
    }
}

/// The field whose files `source` holds under `dir`, as a module that plays it
/// — or the rule the layout breaks, as [`Map::load`] names it.
///
/// What this game registers as its [`crcbl::registry::ModuleFactory`]: the
/// loader `--scene` runs, so a tool plays exactly the field the game would.
/// The vocabulary it is handed goes unused for the same reason: [`Map::load`]
/// builds this game's own, which is the one `--scene` loads with.
///
/// # Errors
///
/// [`Map::load`]'s refusal, as text.
pub(crate) fn start(
    _: &Registry,
    source: &dyn AssetSource,
    dir: &Path,
) -> Result<Box<dyn GameModule>, String> {
    let map = Map::load(source, dir).map_err(|error| error.to_string())?;
    Ok(Box::new(FieldPlay::new(map)))
}

/// A stage on one field, ticked as solo ticks it, with its creeps mirrored
/// into the world it plays in — see the module docs.
#[derive(Debug)]
struct FieldPlay {
    towers: TowersModule,
    /// Each live creep's body and the entity mirroring it.
    walkers: Vec<(ColliderId, Entity)>,
}

impl FieldPlay {
    /// A fresh run on `map`, with the first build phase running and nothing
    /// mirrored yet.
    fn new(map: Map) -> Self {
        Self {
            towers: TowersModule {
                shared: Arc::new(Mutex::new(Stage::new(Arc::new(map)))),
            },
            walkers: Vec::new(),
        }
    }

    /// Brings the [`WALKERS`] system into line with the stage's creeps: an
    /// entity for each creep that arrived, every mirrored creep where it now
    /// is, and a despawn for each that is gone. The world sweeps the despawned
    /// after the module's tick, as a server's does.
    fn mirror(&mut self, world: &mut World) {
        let creeps: Vec<(ColliderId, DVec3)> = lock(&self.towers.shared)
            .creeps
            .iter()
            .map(|creep| (creep.body(), creep.centre()))
            .collect();
        self.walkers.retain(|(body, entity)| {
            let alive = creeps.iter().any(|(live, _)| live == body);
            if !alive {
                world.despawn(*entity);
            }
            alive
        });
        let mut rows = Vec::with_capacity(creeps.len());
        for (body, centre) in creeps {
            let known = self
                .walkers
                .iter()
                .find(|(walker, _)| *walker == body)
                .map(|(_, entity)| *entity);
            let entity = known.unwrap_or_else(|| {
                let entity = world.spawn();
                self.walkers.push((body, entity));
                entity
            });
            rows.push((entity, Walker { centre }));
        }
        let system = world
            .system_mut::<System<Walker>>()
            .expect("`register` put the walkers in the world before the first tick");
        for (entity, walker) in rows {
            system.attach(entity, walker);
        }
    }
}

impl GameModule for FieldPlay {
    fn name(&self) -> &str {
        self.towers.name()
    }

    /// Sets the world to this game's rate — the stage's rules are written for
    /// it — and registers the system the creeps are mirrored into.
    fn register(&self, world: &mut World) {
        world.set_tick_dt(1.0 / f64::from(DEFAULT_TICK_HZ));
        world.register_system(Box::new(System::<Walker>::new(WALKERS)));
    }

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        GameModule::tick(&mut self.towers, world, inputs);
        self.mirror(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::scene::{FIELD, built_in_source};
    use crate::wave::{GAP_S, STARTING_LIVES, WAVES};

    /// A played field in a world of its own, registered as a tool registers it.
    fn played() -> (FieldPlay, World) {
        let module = FieldPlay::new(Map::built_in());
        let mut world = World::new();
        module.register(&mut world);
        (module, world)
    }

    /// One tick in the order a tool's play runs it: the world's schedule, the
    /// module with nothing from any client, then a sweep.
    fn tick(module: &mut FieldPlay, world: &mut World) {
        world.tick();
        module.tick(world, ClientInputs::empty());
        world.sweep();
    }

    /// How many ticks the first build phase lasts at this game's rate: every
    /// one of them runs before the stage's clock reaches the first wave.
    fn build_phase_ticks() -> u32 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = (GAP_S * f64::from(DEFAULT_TICK_HZ)).ceil() as u32;
        ticks
    }

    /// The ticks past the build phase by which the first creep is out: the
    /// tick that releases it, and one more, because the stage's clock is a
    /// sum of tick periods and may fall a rounding short of the phase's end.
    const RELEASE_SLACK: u32 = 2;

    /// How many ticks the stage takes, at most, to release its first creep.
    fn ticks_to_the_first_creep() -> u32 {
        build_phase_ticks() + RELEASE_SLACK
    }

    /// The centres of every mirrored creep.
    fn walkers(world: &mut World) -> Vec<DVec3> {
        world
            .system_mut::<System<Walker>>()
            .expect("registered")
            .iter()
            .map(|walker| walker.centre)
            .collect()
    }

    /// **A played field runs with no client at all**: the build phase runs
    /// down and the first creep is mirrored into the world the tick it is
    /// released, and not before — a module that ticked the stage with no
    /// player would hold it still.
    #[test]
    fn a_played_field_releases_its_first_creep_with_nobody_sending_commands() {
        let (mut module, mut world) = played();
        for _ in 0..build_phase_ticks() {
            tick(&mut module, &mut world);
        }
        assert!(
            walkers(&mut world).is_empty(),
            "a creep before the build phase ran out"
        );
        for _ in 0..RELEASE_SLACK {
            tick(&mut module, &mut world);
        }
        let first = walkers(&mut world);
        assert_eq!(first.len(), 1, "the first release was not mirrored");
        assert_eq!(
            world.entity_count(),
            1,
            "the mirror spawned something other than the creep",
        );
    }

    /// **Mirrored creeps walk the lane and leak, the lives drop, and a leaked
    /// creep's entity leaves the world** — so what is drawn is what the stage
    /// holds, every tick of the first wave's walk.
    #[test]
    fn mirrored_creeps_walk_and_leak_and_the_lives_drop() {
        let (mut module, mut world) = played();
        for _ in 0..ticks_to_the_first_creep() {
            tick(&mut module, &mut world);
        }
        let start = walkers(&mut world)[0];
        tick(&mut module, &mut world);
        assert_ne!(
            walkers(&mut world)[0],
            start,
            "the first creep did not walk"
        );

        // Long enough for the slowest creep of the first wave to walk the
        // whole path after the last of them is released.
        let first = WAVES[0];
        let slowest = (0..first.creeps())
            .filter_map(|index| first.kind_at(index))
            .map(|kind| kind.spec().speed)
            .fold(f64::INFINITY, f64::min);
        let span: f64 = (0..first.creeps())
            .filter_map(|index| first.gap_after(index))
            .sum();
        let length = lock(&module.towers.shared).map.path().length();
        let seconds = span + length / slowest;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let ticks = (seconds * f64::from(DEFAULT_TICK_HZ)).ceil() as u32;
        let mut leaked = false;
        for _ in 0..ticks {
            tick(&mut module, &mut world);
            let stage = lock(&module.towers.shared);
            assert_eq!(
                walkers(&mut world).len(),
                stage.creeps.len(),
                "the mirror and the stage disagree on how many creeps there are",
            );
            assert_eq!(
                world.entity_count(),
                stage.creeps.len(),
                "a creep that left the stage left its entity behind",
            );
            leaked |= stage.leaks > 0;
        }
        assert!(leaked, "no creep reached the exit");
        let lives = lock(&module.towers.shared).lives;
        assert!(lives < STARTING_LIVES, "a leak cost no life");
    }

    /// **A field that breaks the rules is refused by the rule it breaks**,
    /// through the loader `--scene` runs, rather than played.
    #[test]
    fn a_field_that_breaks_the_rules_is_refused_by_name() {
        let mut source = built_in_source();
        let key = format!("{FIELD}/sys/waypoints.ron");
        let text =
            String::from_utf8(source.read(Path::new(&key)).expect("committed")).expect("utf-8");
        let bent = text.replace("(8.0, 0.0, -6.0)", "(2.0, 0.0, -6.0)");
        assert_ne!(
            bent, text,
            "the corner to move is not in the committed file"
        );
        source
            .insert(Path::new(&key), bent.into_bytes())
            .expect("a legal key");

        let refusal = start(&Registry::new(), &source, Path::new(FIELD))
            .err()
            .expect("a diagonal leg is not a lane");
        assert!(refusal.contains("neither X nor Z"), "{refusal}");
        assert!(
            start(&Registry::new(), &built_in_source(), Path::new(FIELD)).is_ok(),
            "the committed field is refused"
        );
    }

    /// A creep is drawn as the box around its sphere.
    #[test]
    fn a_walker_is_placed_as_the_box_around_its_creep() {
        let walker = Walker {
            centre: DVec3::new(1.0, CREEP_RADIUS, -2.0),
        };
        assert_eq!(
            walker.placement(),
            Some(OrientedBox::axis_aligned(
                walker.centre,
                DVec3::splat(CREEP_RADIUS)
            )),
        );
    }
}
