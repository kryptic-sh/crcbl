//! Play mode on towers' committed field, through the shipped vocabulary: the
//! module towers registers runs the game, its creeps walk the lane as
//! entities the document draws and never lists or saves, and stop leaves the
//! scene exactly as it was.
//!
//! Every tick count here is derived from towers' own constants — the build
//! phase, the tick rate, the creep table, the path — rather than written as a
//! number, so a retuned game moves these tests with it.

use std::collections::HashMap;
use std::time::Duration;

use super::*;

use crcbl_towers::map::{CREEP_RADIUS, EXIT_HALF, LANE_WIDTH};
use crcbl_towers::wave::GAP_S;
use crcbl_towers::{CREEPS, DEFAULT_TICK_HZ, FIELD, Map, WAVES};

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
