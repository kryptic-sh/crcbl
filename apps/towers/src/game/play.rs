//! The stage, played in a tool's world: the module `apps/editor`'s play mode
//! runs on this game's field.
//!
//! ```text
//!   the scene's files ──▶ Map::load ──▶ Stage ──▶ TowersModule::tick, as the
//!                                         │         one local player
//!                                         ├──▶ every creep, tower, bolt and
//!                                         │    burst, as runtime entities
//!                                         └──▶ the readout `controls` reads
//! ```
//!
//! # The game's own tick, as one local player
//!
//! `run_team_tick` holds the run still on a tick with no command frame at all —
//! a dedicated server nobody has joined. So this module ticks through
//! [`TowersModule`]'s [`GameModule`] half, which is solo's: it reads the
//! [`ClientInputs`] it is handed as the frames of the one local player, and an
//! empty set is that player asking for nothing this tick — `run_tick` hands
//! `run_team_tick` one empty command, not none. The build phase runs down, the
//! waves come, the creeps walk and leak, a lost run starts itself again: the
//! run solo plays, by the same rules, because it is the same call.
//!
//! **A tool's commands are that player's.** The frames a tool hands the module
//! are the bytes `controls` encodes, which are the bytes solo's client seals
//! for the same command; the stage validates them as it validates any client's
//! and records a refusal, which `controls` hands the tool to tell.
//!
//! # What the stage holds, as entities a tool can draw
//!
//! The stage keeps its creeps, towers, bolts and bursts outside the ECS — see
//! `crate::game`'s module docs — and a tool draws entities. So after every
//! tick this module mirrors each into a runtime system of its own
//! ([`crcbl::registry::Registry::register_runtime`]): a tool draws them from
//! their placements and never lists, edits or saves them.
//!
//! A creep is a [`Walker`] in [`WALKERS`], a tower a [`Turret`] in
//! [`TURRETS`], a bolt a [`Shot`] in [`SHOTS`] and a burst a [`Blast`] in
//! [`BLASTS`] — each **keyed by what it is**, so one thing is one entity for
//! as long as it lives: a creep by its physics body, a tower by its plot (a
//! plot holds one), a bolt by its [`Bolt::id`](crate::tower::Bolt::id) and
//! a burst by the id of the bolt that raised it, every key beside the run it
//! belongs to, since a restart numbers everything again. Not by place: the
//! stage swap-removes a creep that dies and a bolt that lands, and drops the
//! oldest burst off the front of its list, so a place stands for a different
//! thing from tick to tick — a picture that is right every tick and a motion
//! history that is not. A tool's pick of a turret holds the same tower for
//! the same reason.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crcbl::assets::AssetSource;
use crcbl::ecs::{ClientInputs, ComponentHash, Entity, GameModule, System, World};
use crcbl::math::DVec3;
use crcbl::phys::ColliderId;
use crcbl::registry::{OrientedBox, Placement, Registry};

use super::{DEFAULT_TICK_HZ, Stage, TowersModule, lock};
use crate::map::{BOLT_RADIUS, CREEP_RADIUS, Map, TOWER_HEIGHT, TOWER_RADIUS, UPGRADED_SCALE};
use crate::tower::Tier;

mod controls;

/// The runtime system every [`Walker`] is a row of, while the field plays.
const WALKERS: &str = "walkers";

/// The runtime system every [`Turret`] is a row of. Not `towers`, which is
/// this game's [`Registry::group`] label: a tool listing both would read one
/// name as two things.
const TURRETS: &str = "turrets";

/// The runtime system every [`Shot`] is a row of.
const SHOTS: &str = "bolts";

/// The runtime system every [`Blast`] is a row of.
const BLASTS: &str = "bursts";

/// Hashes `values` by their bits, in order: every runtime component here is
/// a few numbers.
fn hash_values(hasher: &mut dyn std::hash::Hasher, values: impl IntoIterator<Item = f64>) {
    for value in values {
        hasher.write(&value.to_bits().to_le_bytes());
    }
}

/// One creep on a played field, as a tool draws it: where its centre is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Walker {
    centre: DVec3,
}

impl ComponentHash for Walker {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hash_values(hasher, self.centre.to_array());
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

/// One built tower on a played field, as a tool draws it: the plot it stands
/// on and where its feet are, and how much bigger than a base tower it
/// stands.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Turret {
    /// The plot's place in the plots chunk: what a command naming this tower
    /// carries, so a tool's pick of the turret becomes the game's
    /// `UpgradeTower`.
    plot: usize,
    feet: DVec3,
    /// 1 for a base tower, [`UPGRADED_SCALE`] for a stepped-up one — the
    /// scale the game's own frame draws it at.
    scale: f64,
}

impl ComponentHash for Turret {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hasher.write_usize(self.plot);
        hash_values(hasher, self.feet.to_array().into_iter().chain([self.scale]));
    }
}

/// The box around the tower's cylinder, standing on its feet: as wide as
/// [`TOWER_RADIUS`] and as tall as [`TOWER_HEIGHT`], both scaled as the frame
/// scales a stepped-up tower.
impl Placement for Turret {
    fn placement(&self) -> Option<OrientedBox> {
        let (radius, half_height) = (TOWER_RADIUS * self.scale, 0.5 * TOWER_HEIGHT * self.scale);
        Some(OrientedBox::axis_aligned(
            self.feet + DVec3::new(0.0, half_height, 0.0),
            DVec3::new(radius, half_height, radius),
        ))
    }
}

/// One bolt in the air on a played field: where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shot {
    centre: DVec3,
}

impl ComponentHash for Shot {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hash_values(hasher, self.centre.to_array());
    }
}

/// The box around the bolt's sphere.
impl Placement for Shot {
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::axis_aligned(
            self.centre,
            DVec3::splat(BOLT_RADIUS),
        ))
    }
}

/// One splash burst on a played field, while the game still draws it: where
/// the bolt stopped, and how far the burst reached.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Blast {
    centre: DVec3,
    radius_m: f64,
}

impl ComponentHash for Blast {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        hash_values(
            hasher,
            self.centre.to_array().into_iter().chain([self.radius_m]),
        );
    }
}

/// The box around the burst's sphere — the overlap that wounded, so the
/// picture is the query, as the game's own frame draws it.
impl Placement for Blast {
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::axis_aligned(
            self.centre,
            DVec3::splat(self.radius_m),
        ))
    }
}

/// Registers every runtime component this module mirrors into, under its
/// system, and the controls a tool plays the field with: what this game's
/// vocabulary adds so a tool can draw a played field and take part in it.
pub(crate) fn register_play(registry: &mut Registry, system: &str) {
    registry.module(system, start);
    registry.play_controls(system, controls::CONTROLS);
    registry.register_runtime::<Walker>(WALKERS);
    registry.register_runtime::<Turret>(TURRETS);
    registry.register_runtime::<Shot>(SHOTS);
    registry.register_runtime::<Blast>(BLASTS);
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
fn start(
    _: &Registry,
    source: &dyn AssetSource,
    dir: &Path,
) -> Result<Box<dyn GameModule>, String> {
    let map = Map::load(source, dir).map_err(|error| error.to_string())?;
    Ok(Box::new(FieldPlay::new(map)))
}

/// A stage on one field, ticked as solo ticks it, with what it holds mirrored
/// into the world it plays in — see the module docs.
#[derive(Debug)]
struct FieldPlay {
    towers: TowersModule,
    /// Each live creep, by its body.
    walkers: Mirror<ColliderId>,
    /// …each built tower, by its plot.
    turrets: Mirror<usize>,
    /// …each bolt in the air, by its id.
    shots: Mirror<u64>,
    /// …and each burst still drawn, by its bolt's id.
    blasts: Mirror<u64>,
}

/// What one mirrored thing is known by from tick to tick: the run it belongs
/// to — [`Stage`]'s count of them, since a restart numbers everything again —
/// and its own name within that run.
type Identity<K> = (u64, K);

/// The entity mirroring each thing of one kind, by its [`Identity`].
type Mirror<K> = Vec<(Identity<K>, Entity)>;

/// What the stage holds this tick, each row with what it is known by, copied
/// out from under its lock so the world can be written without holding it.
struct Mirrored {
    creeps: Vec<(Identity<ColliderId>, Walker)>,
    turrets: Vec<(Identity<usize>, Turret)>,
    shots: Vec<(Identity<u64>, Shot)>,
    blasts: Vec<(Identity<u64>, Blast)>,
}

impl Mirrored {
    /// What `stage` holds, as the rows a tool draws.
    fn of(stage: &Stage) -> Self {
        let run = stage.runs;
        Self {
            creeps: stage
                .creeps
                .iter()
                .map(|creep| {
                    let walker = Walker {
                        centre: creep.centre(),
                    };
                    ((run, creep.body()), walker)
                })
                .collect(),
            turrets: stage
                .towers
                .iter()
                .map(|tower| {
                    let turret = Turret {
                        plot: tower.plot(),
                        feet: stage.map.plots()[tower.plot()].at(),
                        scale: match tower.tier() {
                            Tier::Base => 1.0,
                            Tier::Upgraded => f64::from(UPGRADED_SCALE),
                        },
                    };
                    ((run, tower.plot()), turret)
                })
                .collect(),
            shots: stage
                .bolts
                .iter()
                .map(|bolt| ((run, bolt.id()), Shot { centre: bolt.at() }))
                .collect(),
            blasts: stage
                .bursts
                .iter()
                .map(|burst| {
                    let blast = Blast {
                        centre: burst.at,
                        radius_m: burst.radius_m,
                    };
                    ((run, burst.id), blast)
                })
                .collect(),
        }
    }
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
            turrets: Vec::new(),
            shots: Vec::new(),
            blasts: Vec::new(),
        }
    }

    /// Brings every runtime system into line with what the stage holds. The
    /// world sweeps what this despawned after the module's tick, as a
    /// server's does.
    fn mirror(&mut self, world: &mut World) {
        let mirrored = Mirrored::of(&lock(&self.towers.shared));
        mirror(world, &mut self.walkers, mirrored.creeps);
        mirror(world, &mut self.turrets, mirrored.turrets);
        mirror(world, &mut self.shots, mirrored.shots);
        mirror(world, &mut self.blasts, mirrored.blasts);
    }
}

/// Brings the runtime system of `T` into line with `rows`, one entity per
/// [`Identity`]: a despawn for each thing that is gone, an entity for each
/// that arrived, and every mirrored thing given its row as it now is — so an
/// entity stands for one thing from the tick it arrives to the tick it goes.
fn mirror<K, T>(world: &mut World, known: &mut Mirror<K>, rows: Vec<(Identity<K>, T)>)
where
    K: Copy + PartialEq,
    T: ComponentHash + 'static,
{
    known.retain(|(identity, entity)| {
        let alive = rows.iter().any(|(live, _)| live == identity);
        if !alive {
            world.despawn(*entity);
        }
        alive
    });
    let mut placed = Vec::with_capacity(rows.len());
    for (identity, row) in rows {
        let entity = known
            .iter()
            .find(|(each, _)| *each == identity)
            .map(|(_, entity)| *entity)
            .unwrap_or_else(|| {
                let entity = world.spawn();
                known.push((identity, entity));
                entity
            });
        placed.push((entity, row));
    }
    let system = world
        .system_mut::<System<T>>()
        .expect("`register` put every mirrored system in the world before the first tick");
    for (entity, row) in placed {
        system.attach(entity, row);
    }
}

impl GameModule for FieldPlay {
    fn name(&self) -> &str {
        self.towers.name()
    }

    /// Sets the world to this game's rate — the stage's rules are written for
    /// it — and registers the systems the stage is mirrored into and the
    /// readout a tool's controls read.
    fn register(&self, world: &mut World) {
        world.set_tick_dt(1.0 / f64::from(DEFAULT_TICK_HZ));
        world.register_system(Box::new(System::<Walker>::new(WALKERS)));
        world.register_system(Box::new(System::<Turret>::new(TURRETS)));
        world.register_system(Box::new(System::<Shot>::new(SHOTS)));
        world.register_system(Box::new(System::<Blast>::new(BLASTS)));
        world.register_system(controls::readout(Arc::clone(&self.towers.shared)));
    }

    fn tick(&mut self, world: &mut World, inputs: ClientInputs<'_>) {
        GameModule::tick(&mut self.towers, world, inputs);
        controls::bound_untold(world);
        self.mirror(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::scene::{FIELD, built_in_source};
    use crate::tower;
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

    /// How many ticks the first wave takes, from its first release, for its
    /// slowest creep to walk the whole path after the last of them is
    /// released.
    fn first_wave_ticks(module: &FieldPlay) -> u32 {
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
        ticks
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

        let mut leaked = false;
        for _ in 0..first_wave_ticks(&module) {
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

    /// One tick as a tool's play runs it, with `frame` arriving from its one
    /// player.
    fn tick_with(module: &mut FieldPlay, world: &mut World, frame: Vec<u8>) {
        let frames = [(crcbl::core::TickId::ZERO, frame)];
        world.tick();
        module.tick(world, ClientInputs::new(&frames, 0));
        world.sweep();
    }

    /// The frame this game's controls encode for `action` taking `args`,
    /// read against `world`.
    fn command(world: &mut World, action: &str, args: &[crcbl::registry::PlayArg]) -> Vec<u8> {
        let index = controls::CONTROLS
            .actions
            .iter()
            .position(|each| each.name == action)
            .expect("an action towers offers");
        (controls::CONTROLS.encode)(world, index, args).expect("towers spells it")
    }

    /// One tick with `action` taking `args` arriving from the one player.
    fn tick_command(
        module: &mut FieldPlay,
        world: &mut World,
        action: &str,
        args: &[crcbl::registry::PlayArg],
    ) {
        let frame = command(world, action, args);
        tick_with(module, world, frame);
    }

    /// Every mirrored row of `T`, by entity.
    fn rows<T: ComponentHash + Copy + 'static>(world: &mut World) -> Vec<(Entity, T)> {
        world
            .system_mut::<System<T>>()
            .expect("registered")
            .iter_entities()
            .map(|(entity, row)| (entity, *row))
            .collect()
    }

    /// **A tower a tool's command builds is mirrored where its plot is, at
    /// its tier's size** — and stepping it up, by picking that entity, grows
    /// the same entity rather than adding one.
    #[test]
    fn a_built_tower_is_mirrored_on_its_plot_and_grows_when_stepped_up() {
        use crcbl::registry::PlayArg;

        let (mut module, mut world) = played();
        let plot = 1;
        tick_command(
            &mut module,
            &mut world,
            "Place tower",
            &[PlayArg::Picked(plot), PlayArg::Choice(0)],
        );
        let feet = Map::built_in().plots()[plot].at();
        let built = rows::<Turret>(&mut world);
        assert_eq!(
            built.iter().map(|(_, turret)| *turret).collect::<Vec<_>>(),
            [Turret {
                plot,
                feet,
                scale: 1.0
            }],
        );
        let placed = built[0].1.placement().expect("a tower is in space");
        assert!(
            (placed.centre.y - 0.5 * TOWER_HEIGHT).abs() < 1e-12
                && (placed.half_extents.x - TOWER_RADIUS).abs() < 1e-12,
            "a base tower is drawn as {placed:?}",
        );

        tick_command(
            &mut module,
            &mut world,
            "Upgrade",
            &[PlayArg::PickedRuntime(built[0].0)],
        );
        let stepped = rows::<Turret>(&mut world);
        assert_eq!(stepped.len(), 1, "an upgrade mirrored a second tower");
        assert_eq!(
            stepped[0].0, built[0].0,
            "an upgrade moved to another entity"
        );
        assert_eq!(stepped[0].1.scale, f64::from(UPGRADED_SCALE));
    }

    /// **Bolts and bursts are mirrored for as long as the stage holds them,
    /// tick by tick, and leave with them**: a splash tower on the lane's
    /// side, through the first wave, with every entity in the world one of
    /// the stage's four lists.
    #[test]
    fn bolts_and_bursts_are_mirrored_while_the_stage_holds_them() {
        use crcbl::registry::PlayArg;

        let (mut module, mut world) = played();
        let splash = tower::ALL
            .iter()
            .position(|kind| *kind == tower::Kind::Splash)
            .expect("towers has a splash tower");
        tick_command(
            &mut module,
            &mut world,
            "Place tower",
            &[PlayArg::Picked(0), PlayArg::Choice(splash)],
        );
        let (mut shot, mut blasted) = (false, false);
        for _ in 0..ticks_to_the_first_creep() + first_wave_ticks(&module) {
            tick(&mut module, &mut world);
            let (bolts, bursts, held) = {
                let stage = lock(&module.towers.shared);
                let held = stage.creeps.len() + stage.towers.len();
                (stage.bolts.clone(), stage.bursts.clone(), held)
            };
            let shots = rows::<Shot>(&mut world);
            let blasts = rows::<Blast>(&mut world);
            assert_eq!(shots.len(), bolts.len(), "the mirror lost or kept a bolt");
            assert_eq!(
                blasts.len(),
                bursts.len(),
                "the mirror lost or kept a burst"
            );
            for (_, mirrored) in &shots {
                assert!(
                    bolts.iter().any(|bolt| bolt.at() == mirrored.centre),
                    "a bolt is drawn where none is"
                );
            }
            for (_, mirrored) in &blasts {
                assert!(
                    bursts
                        .iter()
                        .any(|burst| (burst.at, burst.radius_m)
                            == (mirrored.centre, mirrored.radius_m)),
                    "a burst is drawn where none is"
                );
            }
            assert_eq!(
                world.entity_count(),
                held + bolts.len() + bursts.len(),
                "the world holds an entity the stage does not",
            );
            shot |= !bolts.is_empty();
            blasted |= !bursts.is_empty();
        }
        assert!(shot && blasted, "the splash tower never fired and burst");
    }

    /// The ids of what one of the stage's lists holds, in the list's order.
    fn ids<T>(list: &[T], id: impl Fn(&T) -> u64) -> Vec<u64> {
        list.iter().map(id).collect()
    }

    /// Whether some place in one of the stage's lists holds another thing
    /// than it held a tick before — what a mirror by place would draw as one
    /// entity leaving one thing for the next.
    fn places_moved(before: &[u64], now: &[u64]) -> bool {
        before.iter().zip(now).any(|(was, is)| was != is)
    }

    /// **A bolt or a burst keeps its entity when the stage reorders its
    /// lists**: three bolts dropped on the first creep from heights that land
    /// the first of them first, which the stage swap-removes from under the
    /// other two, and two bursts of which the older expires first, off the
    /// front of the list. A bolt's entity moves no further in a tick than a
    /// bolt flies and a burst's never moves, where a mirror by place would
    /// draw each reordering as a jump.
    #[test]
    fn bolts_and_bursts_keep_their_entities_when_the_stage_reorders_its_lists() {
        use std::collections::HashMap;

        use crate::game::Burst;
        use crate::tower::{BOLT_SPEED, BURST_S, Bolt, Kind};

        /// How far above the creep each bolt is dropped from, in metres, by
        /// its id: the first lands first, a tick or two after the tick that
        /// first mirrors them all.
        const DROPS_M: [f64; 3] = [4.0, 9.0, 13.0];

        let (mut module, mut world) = played();
        let start = command(&mut world, "Start wave", &[]);
        tick_with(&mut module, &mut world, start);
        for _ in 0..RELEASE_SLACK {
            tick(&mut module, &mut world);
        }
        {
            let mut stage = lock(&module.towers.shared);
            let creep = stage
                .creeps
                .first()
                .expect("the sent wave released a creep");
            let (at, spec) = (creep.centre(), Kind::Bolt.spec(Tier::Base));
            let bolts: Vec<Bolt> = (0..)
                .zip(DROPS_M)
                .map(|(id, height)| Bolt::fire(id, at + DVec3::Y * height, creep, spec))
                .collect();
            stage.bolts.extend(bolts);
            // Two metres apart, the older half spent, so it expires first.
            let elapsed = stage.elapsed;
            for (id, raised_at, aside_m) in [(0, elapsed - 0.5 * BURST_S, 0.0), (1, elapsed, 2.0)] {
                stage.bursts.push(Burst {
                    id,
                    at: at + DVec3::X * aside_m,
                    radius_m: 1.0,
                    raised_at,
                });
            }
        }

        // One tick to mirror them all, so every reordering after it is
        // between two mirrored ticks.
        tick(&mut module, &mut world);
        let flown = BOLT_SPEED / f64::from(DEFAULT_TICK_HZ);
        let mut shots_before: HashMap<Entity, Shot> =
            rows::<Shot>(&mut world).into_iter().collect();
        let mut blasts_before: HashMap<Entity, Blast> =
            rows::<Blast>(&mut world).into_iter().collect();
        assert_eq!(
            (shots_before.len(), blasts_before.len()),
            (DROPS_M.len(), 2),
            "a dropped bolt or a burst was gone before it was mirrored",
        );
        let mut before = {
            let stage = lock(&module.towers.shared);
            (
                ids(&stage.bolts, Bolt::id),
                ids(&stage.bursts, |burst| burst.id),
            )
        };
        let (mut bolts_moved, mut bursts_moved) = (false, false);
        // A second is far longer than the highest drop's fall or a burst's
        // life: a run past it is a bolt that never landed.
        for ticks in 0.. {
            assert!(
                ticks < DEFAULT_TICK_HZ,
                "a bolt or a burst outlived a second"
            );
            tick(&mut module, &mut world);
            let now = {
                let stage = lock(&module.towers.shared);
                (
                    ids(&stage.bolts, Bolt::id),
                    ids(&stage.bursts, |burst| burst.id),
                )
            };
            let shots: HashMap<Entity, Shot> = rows::<Shot>(&mut world).into_iter().collect();
            let blasts: HashMap<Entity, Blast> = rows::<Blast>(&mut world).into_iter().collect();
            for (entity, shot) in &shots {
                if let Some(was) = shots_before.get(entity) {
                    let step = (shot.centre - was.centre).length();
                    assert!(
                        step <= flown * (1.0 + 1e-9),
                        "a bolt's entity jumped {step} m in a tick, past the {flown} m a \
                         bolt flies",
                    );
                }
            }
            for (entity, blast) in &blasts {
                if let Some(was) = blasts_before.get(entity) {
                    assert_eq!(was, blast, "a burst's entity moved to another burst");
                }
            }
            bolts_moved |= places_moved(&before.0, &now.0);
            bursts_moved |= places_moved(&before.1, &now.1);
            if now.0.is_empty() && now.1.is_empty() {
                break;
            }
            before = now;
            (shots_before, blasts_before) = (shots, blasts);
        }
        assert!(
            bolts_moved && bursts_moved,
            "the stage never reordered its lists (bolts {bolts_moved}, bursts \
             {bursts_moved}), so nothing here tells a mirror by identity from one by place",
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
