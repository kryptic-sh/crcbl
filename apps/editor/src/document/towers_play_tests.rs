//! Play mode on towers' committed field, through the shipped vocabulary: the
//! module towers registers runs the game, its creeps walk the lane as
//! entities the document draws and never lists or saves, towers placed
//! through its play controls stand on their plots and cost their price, and
//! stop leaves the scene exactly as it was.
//!
//! Every tick count here is derived from towers' own constants — the build
//! phase, the tick rate, the creep table, the path — rather than written as a
//! number, so a retuned game moves these tests with it.

use std::collections::HashMap;
use std::time::Duration;

use super::*;

use crcbl::registry::PlayArg;
use crcbl_towers::game::Refusal;
use crcbl_towers::map::{CREEP_RADIUS, EXIT_HALF, LANE_WIDTH, TOWER_HEIGHT, TOWER_RADIUS};
use crcbl_towers::tower::Kind;
use crcbl_towers::wave::{GAP_S, STARTING_GOLD};
use crcbl_towers::{CREEPS, DEFAULT_TICK_HZ, FIELD, Map, Tier, WAVES};

/// The ticks past the build phase by which the first creep is out: the tick
/// that releases it, and one more, because the stage's clock is a sum of tick
/// periods and may fall a rounding short of the phase's end.
const RELEASE_SLACK: u32 = 2;

/// Towers' committed field, opened with the vocabulary the editor ships.
fn field() -> Document {
    Document::open(
        &crcbl_towers::built_in_source(),
        Path::new(FIELD),
        crate::scene::vocabulary(),
    )
    .expect("the shipped vocabulary opens towers' field")
}

/// How many ticks the first build phase lasts at towers' rate.
fn build_phase_ticks() -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = (GAP_S * f64::from(DEFAULT_TICK_HZ)).ceil() as u32;
    ticks
}

/// Hands play `ticks` of the world's tick period, one at a time — so the
/// clock's catch-up cap never drops one — and asserts each ran.
fn run(document: &mut Document, ticks: u32) {
    let period = Duration::try_from_secs_f64(document.world.tick_dt())
        .expect("towers' module set a period play steps at");
    for _ in 0..ticks {
        assert_eq!(document.advance(period), 1, "a tick did not run");
    }
}

/// The field, playing, with the first creep just released.
fn with_the_first_creep() -> Document {
    let mut document = field();
    document.play().expect("towers plays its committed field");
    run(&mut document, build_phase_ticks() + RELEASE_SLACK);
    document
}

/// The centre of a spawned entity's bounds, and its half extents.
fn centre_and_half(document: &mut Document, entity: Entity) -> (Vec3, Vec3) {
    let (min, max) = document
        .spawned_bounds(entity)
        .expect("a spawned creep has bounds");
    ((min + max) * 0.5, (max - min) * 0.5)
}

/// **Towers' module plays its field, and nothing walks during the build
/// phase**: the module is the one towers registered, and no creep is spawned
/// until the phase has run down — with no client sending a thing.
#[test]
fn towers_module_plays_the_field_and_holds_creeps_until_the_build_phase_ends() {
    let mut document = field();
    document.play().expect("towers plays its committed field");
    assert_eq!(document.playing_modules(), ["towers"]);
    run(&mut document, build_phase_ticks());
    assert!(
        document.spawned().is_empty(),
        "a creep walked before the build phase ran out"
    );
    run(&mut document, RELEASE_SLACK);
    assert_eq!(
        document.spawned().len(),
        1,
        "the first wave's first creep was not spawned"
    );
}

/// **A spawned creep is drawn as the box around its sphere, and walks the
/// first leg of the lane** — along it, toward the next corner, and on it.
#[test]
fn a_spawned_creep_is_drawn_and_walks_along_the_lane() {
    let mut document = with_the_first_creep();
    let creep = document.spawned()[0];
    let (start, half) = centre_and_half(&mut document, creep);
    #[allow(clippy::cast_possible_truncation)]
    let radius = CREEP_RADIUS as f32;
    assert!(
        (half - Vec3::splat(radius)).abs().max_element() < 1e-6,
        "a creep is drawn {half} wide, not its radius",
    );

    let map = Map::built_in();
    let [spawn, corner] = [0, 1].map(|index| narrow(map.path().waypoints()[index]));
    let toward = (corner - spawn).normalize();
    run(&mut document, DEFAULT_TICK_HZ);
    let (now, _) = centre_and_half(&mut document, creep);
    let walked = (now - start).dot(toward);
    assert!(
        walked > 0.0,
        "the creep did not walk toward the next corner"
    );
    let off_the_lane = (now - spawn) - toward * (now - spawn).dot(toward);
    #[allow(clippy::cast_possible_truncation)]
    let lane = 0.5 * LANE_WIDTH as f32;
    assert!(
        Vec3::new(off_the_lane.x, 0.0, off_the_lane.z).length() < lane,
        "the creep left the lane: {now}",
    );
}

/// **A creep that reaches the exit leaves the world and the picture**, and
/// where it was last drawn is the exit — no tower stands on the field, so
/// every creep that goes is a leak.
#[test]
fn creeps_that_reach_the_exit_are_no_longer_drawn() {
    let mut document = with_the_first_creep();
    let map = Map::built_in();
    let exit = narrow(map.exit_centre());

    // Long enough for the slowest creep of the first wave to walk the whole
    // path after the last of them is released.
    let first = WAVES[0];
    let slowest = (0..first.creeps())
        .filter_map(|index| first.kind_at(index))
        .map(|kind| kind.spec().speed)
        .fold(f64::INFINITY, f64::min);
    let span: f64 = (0..first.creeps())
        .filter_map(|index| first.gap_after(index))
        .sum();
    let seconds = span + map.path().length() / slowest;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = (seconds * f64::from(DEFAULT_TICK_HZ)).ceil() as u32;

    // How far from the exit's centre a creep can be on the tick it is let
    // through: the volume's reach, the creep's own, and one tick of the
    // fastest walk.
    let fastest = CREEPS.iter().map(|spec| spec.speed).fold(0.0, f64::max);
    #[allow(clippy::cast_possible_truncation)]
    let reach = (EXIT_HALF.x + CREEP_RADIUS + fastest / f64::from(DEFAULT_TICK_HZ)) as f32;

    let mut last: HashMap<Entity, Vec3> = HashMap::new();
    let mut gone = 0;
    for _ in 0..ticks {
        run(&mut document, 1);
        let now: HashMap<Entity, Vec3> = document
            .spawned()
            .into_iter()
            .map(|entity| (entity, centre_and_half(&mut document, entity).0))
            .collect();
        for (entity, centre) in &last {
            if now.contains_key(entity) {
                continue;
            }
            gone += 1;
            let away = Vec3::new(centre.x - exit.x, 0.0, centre.z - exit.z).length();
            assert!(
                away <= reach,
                "a creep left the field {away} m from the exit"
            );
            assert!(
                document.spawned_bounds(*entity).is_none(),
                "a creep that left is still drawn",
            );
        }
        last = now;
    }
    assert!(gone > 0, "no creep reached the exit");
}

/// **Stop takes every spawned creep away with the world they walked in, and
/// the files are byte for byte the ones play began with** — and they were the
/// same all through play, because the creeps are no row of the scene's.
#[test]
fn stop_removes_every_creep_and_restores_the_files() {
    let mut document = field();
    let before = document.files().expect("every row has an id");
    document.play().expect("plays");
    run(&mut document, build_phase_ticks() + RELEASE_SLACK);
    assert!(
        !document.spawned().is_empty(),
        "nothing walked, so stop proves nothing"
    );
    assert_eq!(
        document.files().expect("a playing field still saves"),
        before,
        "the creeps reached the scene's files",
    );

    assert!(document.stop().expect("the snapshot loads again"));
    assert!(document.spawned().is_empty(), "a creep outlived play");
    assert_eq!(
        document.world.entity_count(),
        document.entity_count(),
        "the world holds entities the scene does not",
    );
    assert_eq!(document.files().expect("ids"), before);
}

/// **A spawned creep is not listed, not counted and not picked**: the
/// outline is the manifest's systems and their rows, and a ray straight down
/// through a creep standing on the spawn corner picks the corner.
#[test]
fn spawned_creeps_are_not_listed_counted_or_picked() {
    let mut editing = field();
    let outline = editing.outline();
    let count = editing.entity_count();

    let mut document = with_the_first_creep();
    assert_eq!(document.outline(), outline, "a creep reached the outline");
    assert_eq!(document.entity_count(), count, "a creep was counted");

    let creep = document.spawned()[0];
    let (centre, _) = centre_and_half(&mut document, creep);
    let spawn = narrow(Map::built_in().path().waypoints()[0]);
    #[allow(clippy::cast_possible_truncation)]
    let corner = 0.5 * LANE_WIDTH as f32;
    assert!(
        (centre.x - spawn.x).abs() < corner && (centre.z - spawn.z).abs() < corner,
        "the creep has walked off the spawn corner, so the ray below proves nothing",
    );
    let down = Ray::new(widen(centre + Vec3::Y * 10.0), DVec3::NEG_Y);
    let spawn_corner = outline
        .iter()
        .find(|(system, _)| system == "waypoints")
        .map(|(_, ids)| ids[0])
        .expect("the field has a path");
    assert_eq!(
        document.pick(&down),
        Some(spawn_corner),
        "the creep took the click from the corner it stands on",
    );
    let corner_entity = document.ids.entity(spawn_corner).expect("in the scene");
    assert_eq!(
        document.spawned_bounds(corner_entity),
        None,
        "a scene entity answered as a spawned one",
    );
}

/// **A field that breaks towers' rules does not play, and says which rule** —
/// the document is left editing, with the edit that broke it, and undoing the
/// edit lets it play.
#[test]
fn a_field_that_breaks_the_rules_refuses_play_by_name() {
    let mut document = field();
    let (_, waypoints) = document
        .outline()
        .into_iter()
        .find(|(system, _)| system == "waypoints")
        .expect("the field has a path");
    // The third corner pulled off the second's line, so the leg between them
    // runs along neither axis.
    document
        .apply(EditCommand::SetProperty {
            entity: waypoints[2],
            system: "waypoints".to_owned(),
            path: "position.0".to_owned(),
            value: Value::Float(2.0),
        })
        .expect("a waypoint has an x");
    let edited = document.files().expect("ids");

    let error = document.play().expect_err("a diagonal leg is not a lane");
    assert!(matches!(error, EditError::Unplayable(_)), "{error}");
    assert!(error.to_string().contains("neither X nor Z"), "{error}");
    assert_eq!(document.play_state(), PlayState::Editing);
    assert!(document.playing_modules().is_empty());
    assert_eq!(document.files().expect("ids"), edited);
    assert!(document.is_dirty(), "the refusal threw the edit away");

    document.undo().expect("the edit undoes");
    document.play().expect("the committed layout plays");
    assert_eq!(document.playing_modules(), ["towers"]);
}

/// **Towers' module runs only on a scene that holds a path**: breakout's
/// board, puppet's blockout and the editor's own greybox play with no module
/// at all through the same shipped vocabulary.
#[test]
fn scenes_of_other_games_do_not_run_towers_module() {
    let others = [
        (crcbl_breakout::built_in_source(), crcbl_breakout::BOARD),
        (
            crcbl_puppet::map::built_in_source(),
            crcbl_puppet::map::BLOCKOUT,
        ),
        (crate::scene::built_in_source(), crate::scene::GREYBOX),
    ];
    for (source, dir) in others {
        let mut document = Document::open(&source, Path::new(dir), crate::scene::vocabulary())
            .expect("the shipped vocabulary opens every sample's scene");
        document.play().expect("a scene no game refuses plays");
        assert!(
            document.playing_modules().is_empty(),
            "{dir} ran {:?}",
            document.playing_modules(),
        );
        run(&mut document, 1);
        assert!(document.spawned().is_empty(), "{dir} spawned creeps");
    }
}

/// The system towers' module and play controls are registered under.
const TOWERS_SYSTEM: &str = "waypoints";

/// The index of the action named `name` in towers' play controls.
fn action(document: &Document, name: &str) -> usize {
    let controls = document.play_controls();
    let (system, controls) = controls.first().expect("towers offers play controls");
    assert_eq!(*system, TOWERS_SYSTEM);
    controls
        .actions
        .iter()
        .position(|each| each.name == name)
        .expect("an action towers offers")
}

/// The `index`th plot of the field, in file order, selected.
fn select_plot(document: &mut Document, index: usize) -> Vec3 {
    let (_, plots) = document
        .outline()
        .into_iter()
        .find(|(system, _)| system == "plots")
        .expect("the field has plots");
    document.select(Some(plots[index]));
    assert_eq!(document.picked("plots"), Some(index));
    narrow(Map::built_in().plots()[index].at())
}

/// The value the run's status gives `label`.
fn status_of(document: &mut Document, label: &str) -> String {
    document
        .play_status()
        .into_iter()
        .find(|(each, _)| *each == label)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("the status has no {label}"))
}

/// What a base tower of `kind` costs.
fn cost(kind: Kind) -> u32 {
    kind.spec(Tier::Base).cost
}

/// Sends `Place tower` on the selected plot, of the kind at `kind` in
/// towers' table.
fn place(document: &mut Document, kind: usize) {
    let plot = document.picked("plots").expect("a plot is selected");
    let index = action(document, "Place tower");
    document
        .send_play(
            TOWERS_SYSTEM,
            index,
            &[PlayArg::Picked(plot), PlayArg::Choice(kind)],
        )
        .expect("towers encodes a build");
}

/// The spawned entities whose box stands on `feet`, across the ground.
fn standing_on(document: &mut Document, feet: Vec3) -> Vec<Entity> {
    document
        .spawned()
        .into_iter()
        .filter(|&entity| {
            let (centre, _) = centre_and_half(document, entity);
            (centre.x - feet.x).abs() < 1e-4 && (centre.z - feet.z).abs() < 1e-4
        })
        .collect()
}

/// **A tower placed through towers' play controls stands on the selected
/// plot and is drawn, and the purse pays its price** — the command reaching
/// the module on its next tick as a client's would, and nothing before it.
#[test]
fn a_tower_placed_through_the_controls_stands_on_its_plot_and_costs_its_price() {
    let mut document = field();
    document.play().expect("towers plays its committed field");
    let feet = select_plot(&mut document, 0);
    assert_eq!(status_of(&mut document, "Gold"), STARTING_GOLD.to_string());

    place(&mut document, Kind::Bolt.index());
    assert!(
        standing_on(&mut document, feet).is_empty(),
        "a tower stood before a tick read the command",
    );
    run(&mut document, 1);
    let towers = standing_on(&mut document, feet);
    assert_eq!(towers.len(), 1, "the tower is not drawn on its plot");
    let (centre, half) = centre_and_half(&mut document, towers[0]);
    #[allow(clippy::cast_possible_truncation)]
    let (radius, height) = (TOWER_RADIUS as f32, TOWER_HEIGHT as f32);
    assert!(
        (half.x - radius).abs() < 1e-5 && (centre.y - 0.5 * height).abs() < 1e-5,
        "a tower is drawn as {centre} reaching {half}",
    );
    assert_eq!(
        status_of(&mut document, "Gold"),
        (STARTING_GOLD - cost(Kind::Bolt)).to_string(),
    );

    // Read by one tick and then gone: a frame handed to the next tick too
    // would build on the taken plot again and be refused.
    run(&mut document, 1);
    assert!(
        document.take_play_refusals().is_empty(),
        "the command was read twice"
    );
    assert_eq!(
        status_of(&mut document, "Gold"),
        (STARTING_GOLD - cost(Kind::Bolt)).to_string(),
    );
}

/// **A command reaches the module of the system it was sent under and no
/// other**: a recording module registered under towers' `plots`, ahead of
/// towers' own, is handed nothing when towers is sent a build.
#[test]
fn a_command_reaches_only_the_module_it_was_sent_to() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crcbl::ecs::{ClientInputs, GameModule, World};

    /// How many frames [`Recorder`] has been handed.
    static HANDED: AtomicUsize = AtomicUsize::new(0);

    struct Recorder;

    impl GameModule for Recorder {
        fn name(&self) -> &str {
            "recorder"
        }
        fn register(&self, _world: &mut World) {}
        fn tick(&mut self, _world: &mut World, inputs: ClientInputs<'_>) {
            HANDED.fetch_add(inputs.len(), Ordering::Relaxed);
        }
    }

    let mut registry = Registry::new();
    registry.module("plots", |_, _, _| Ok(Box::new(Recorder)));
    crcbl_towers::register_components(&mut registry);
    let mut document = Document::open(&crcbl_towers::built_in_source(), Path::new(FIELD), registry)
        .expect("towers' vocabulary opens its field");
    document.play().expect("plays");
    assert_eq!(document.playing_modules(), ["recorder", "towers"]);
    let feet = select_plot(&mut document, 0);
    place(&mut document, Kind::Bolt.index());
    run(&mut document, 1);
    assert_eq!(
        HANDED.load(Ordering::Relaxed),
        0,
        "towers' command reached another module"
    );
    assert_eq!(
        standing_on(&mut document, feet).len(),
        1,
        "towers did not build"
    );
}

/// **A build the rules refuse is refused by the game, which says why**: a
/// second tower on a taken plot, and one the purse cannot pay for — and
/// neither costs a coin.
#[test]
fn a_refused_build_says_why_and_costs_nothing() {
    let mut document = field();
    document.play().expect("plays");
    select_plot(&mut document, 0);
    place(&mut document, Kind::Bolt.index());
    run(&mut document, 1);
    let paid = status_of(&mut document, "Gold");

    place(&mut document, Kind::Bolt.index());
    run(&mut document, 1);
    assert_eq!(document.take_play_refusals(), [Refusal::PlotTaken.label()]);
    assert_eq!(
        status_of(&mut document, "Gold"),
        paid,
        "a refusal was charged"
    );

    // The dearest kind on every free plot, until the purse is short.
    let dearest = crcbl_towers::tower::ALL
        .into_iter()
        .max_by_key(|kind| cost(*kind))
        .expect("towers has tower kinds");
    let plots = Map::built_in().plots().len();
    let mut refused = Vec::new();
    for plot in 1..plots {
        select_plot(&mut document, plot);
        place(&mut document, dearest.index());
        run(&mut document, 1);
        refused.extend(document.take_play_refusals());
        if !refused.is_empty() {
            break;
        }
    }
    assert_eq!(refused, [Refusal::NotEnoughGold.label()]);
}

/// **`Start wave` brings the first wave forward**: the status counts it and
/// a creep walks before the build phase would have run out.
#[test]
fn start_wave_sends_the_first_wave_before_the_build_phase_ends() {
    let mut document = field();
    document.play().expect("plays");
    assert_eq!(
        status_of(&mut document, "Wave"),
        format!("0/{}", WAVES.len())
    );
    let start = action(&document, "Start wave");
    document
        .send_play(TOWERS_SYSTEM, start, &[])
        .expect("towers encodes it");
    run(&mut document, RELEASE_SLACK);
    assert_eq!(
        status_of(&mut document, "Wave"),
        format!("1/{}", WAVES.len())
    );
    assert!(
        !document.spawned().is_empty(),
        "no creep walked after the wave was sent"
    );
    assert!(RELEASE_SLACK < build_phase_ticks());
}

/// **Stop takes the placed towers away with every creep, and the files are
/// byte for byte the ones play began with.**
#[test]
fn stop_after_a_build_restores_the_files_and_leaves_nothing_spawned() {
    let mut document = field();
    let before = document.files().expect("every row has an id");
    document.play().expect("plays");
    let feet = select_plot(&mut document, 0);
    place(&mut document, Kind::Bolt.index());
    run(&mut document, build_phase_ticks() + RELEASE_SLACK);
    assert_eq!(
        standing_on(&mut document, feet).len(),
        1,
        "nothing was built"
    );
    assert_eq!(
        document.files().expect("ids"),
        before,
        "a tower reached the files"
    );

    assert!(document.stop().expect("the snapshot loads again"));
    assert!(document.spawned().is_empty(), "a tower outlived play");
    assert_eq!(document.world.entity_count(), document.entity_count());
    assert_eq!(document.files().expect("ids"), before);
    assert!(
        document.play_status().is_empty(),
        "the status outlived play"
    );
    assert!(document.play_controls().is_empty());
}

/// **A command is sent only to a game that is playing**: none while
/// editing, none to a system no running module is under, and none the
/// controls cannot encode — each refused before anything is queued.
#[test]
fn a_command_is_refused_unless_a_running_game_can_encode_it() {
    let mut document = field();
    let error = document
        .send_play(TOWERS_SYSTEM, 0, &[])
        .expect_err("nothing is playing");
    assert!(matches!(error, EditError::NotPlaying), "{error}");

    document.play().expect("plays");
    let start = action(&document, "Start wave");
    let error = document
        .send_play("plots", start, &[])
        .expect_err("no module runs under plots");
    assert!(matches!(error, EditError::PlayCommand(_)), "{error}");
    let error = document
        .send_play(TOWERS_SYSTEM, start, &[PlayArg::Picked(0)])
        .expect_err("Start wave takes nothing");
    assert!(matches!(error, EditError::PlayCommand(_)), "{error}");

    run(&mut document, RELEASE_SLACK);
    assert_eq!(
        status_of(&mut document, "Wave"),
        format!("0/{}", WAVES.len()),
        "a refused command reached the game",
    );
}

/// The runtime system towers' `Upgrade` picks from, read off its
/// description.
fn upgrade_picks_from(document: &Document) -> &'static str {
    let controls = document.play_controls();
    let (_, controls) = controls.first().expect("towers offers play controls");
    let upgrade = &controls.actions[action(document, "Upgrade")];
    match upgrade.params {
        [crcbl::registry::ParamKind::PickedRuntime(system)] => system,
        other => panic!("Upgrade takes {other:?}, not one picked tower"),
    }
}

/// **A built tower is what a ray down onto its plot hits, it becomes the
/// runtime pick, and `Upgrade` steps that tower up** — the picked entity
/// encoded as the plot it stands on, the purse paying the upgrade's price.
/// The pick names nothing in another runtime system, and goes with the
/// world on stop.
#[test]
fn a_built_tower_is_hit_picked_and_stepped_up_by_upgrade() {
    use crate::document::Hit;

    let mut document = field();
    document.play().expect("plays");
    let feet = select_plot(&mut document, 0);
    place(&mut document, Kind::Bolt.index());
    run(&mut document, 1);
    let [tower] = standing_on(&mut document, feet)[..] else {
        panic!("one tower stands on the plot");
    };
    let paid = STARTING_GOLD - cost(Kind::Bolt);
    assert_eq!(status_of(&mut document, "Gold"), paid.to_string());

    let down = Ray::new(widen(feet + Vec3::Y * 10.0), DVec3::NEG_Y);
    assert_eq!(document.hit(&down), Some(Hit::Spawned(tower)));
    assert_eq!(
        document.pick(&down),
        None,
        "a tower was taken for a scene pick"
    );
    let turrets = upgrade_picks_from(&document);
    assert_eq!(
        document.picked_runtime(turrets),
        None,
        "a pick before a click"
    );
    document.set_runtime_pick(Some(tower));
    assert_eq!(document.picked_runtime(turrets), Some(tower));
    assert_eq!(document.picked_runtime("walkers"), None);

    let upgrade = action(&document, "Upgrade");
    document
        .send_play(TOWERS_SYSTEM, upgrade, &[PlayArg::PickedRuntime(tower)])
        .expect("towers encodes the picked tower");
    let (_, before) = centre_and_half(&mut document, tower);
    run(&mut document, 1);
    assert!(document.take_play_refusals().is_empty());
    let upgraded = Kind::Bolt.spec(Tier::Upgraded).cost;
    assert_eq!(
        status_of(&mut document, "Gold"),
        (paid - upgraded).to_string()
    );
    let (_, after) = centre_and_half(&mut document, tower);
    assert!(after.y > before.y, "the picked tower did not grow");

    assert!(document.stop().expect("restores"));
    document.play().expect("plays again");
    assert_eq!(
        document.picked_runtime(turrets),
        None,
        "a pick outlived play"
    );
}

/// **A command sent while paused is queued, and read on the first tick after
/// resume** — not refused, not dropped, and not read while nothing ticks.
#[test]
fn a_command_sent_while_paused_is_read_on_the_tick_after_resume() {
    let mut document = field();
    document.play().expect("plays");
    assert!(document.pause());
    let start = action(&document, "Start wave");
    document
        .send_play(TOWERS_SYSTEM, start, &[])
        .expect("a paused game takes a command");
    let period = Duration::try_from_secs_f64(document.world.tick_dt()).expect("a period");
    assert_eq!(document.advance(period), 0, "a paused scene ticked");
    assert_eq!(
        status_of(&mut document, "Wave"),
        format!("0/{}", WAVES.len())
    );

    document.play().expect("resumes");
    run(&mut document, 1);
    assert_eq!(
        status_of(&mut document, "Wave"),
        format!("1/{}", WAVES.len()),
        "the command sent while paused was not read on resume",
    );
    assert!(document.take_play_refusals().is_empty());
}
