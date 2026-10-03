//! The walker on the committed field: it lands, walks the lane, steps onto a
//! pad, is stopped by a tower and by the field's edge, and passes through the
//! exit volume.

use super::*;
use crate::game::DEFAULT_TICK_HZ;
use crate::map::{LANE_HEIGHT, PAD_HEIGHT, TOWER_RADIUS};
use crate::tower::{Kind, TowerView};

/// One tick at the default rate.
const DT: f64 = 1.0 / DEFAULT_TICK_HZ as f64;

/// Ticks in `seconds` at the default rate.
fn ticks(seconds: f64) -> usize {
    (seconds * f64::from(DEFAULT_TICK_HZ)).round() as usize
}

/// A walker on the committed field.
fn walker() -> Walker {
    Walker::new(&Map::built_in())
}

/// Where the committed field's plot labelled `label` has its feet, and its
/// index.
fn plot(label: &str) -> (usize, DVec3) {
    let map = Map::built_in();
    let at = map
        .plots()
        .iter()
        .position(|plot| plot.label == label)
        .unwrap_or_else(|| panic!("the map has no {label} plot"));
    (at, map.plots()[at].at())
}

/// A capsule standing with its feet at `feet`.
fn standing_at(walker: &mut Walker, feet: DVec3) {
    let config = *walker.config();
    walker.place(feet + DVec3::Y * standing_centre(&config));
}

/// Walks `direction` for `seconds`, answering what each tick's move did and
/// what its sweeps met.
fn walk(
    walker: &mut Walker,
    direction: DVec3,
    seconds: f64,
) -> Vec<(MoveOutcome, Vec<Option<Part>>)> {
    (0..ticks(seconds))
        .map(|_| {
            let outcome = walker.step(direction, DT);
            let met = walker
                .contacts()
                .iter()
                .map(|contact| walker.part_of(contact.collider))
                .collect();
            (outcome, met)
        })
        .collect()
}

/// Every plot of the committed field empty, but the one at `plot` with a bolt
/// tower of `tier`.
fn one_tower(plot: usize, tier: Tier) -> [Option<TowerView>; MAX_PLOTS] {
    let mut towers = [None; MAX_PLOTS];
    towers[plot] = Some(TowerView {
        kind: Kind::Bolt,
        tier,
        working: false,
    });
    towers
}

/// **A walker dropped over the field falls, lands on the ground and stands
/// there**, a skin width over it — gravity integrated into a fall rather than
/// the ground probe pulling it down, since the drop is far past the step
/// offset the probe reaches.
#[test]
fn a_walker_dropped_over_the_field_falls_to_the_ground_and_stands() {
    let mut walker = walker();
    walker.place(DVec3::new(0.0, 10.0, 0.0));
    let moves = walk(&mut walker, DVec3::ZERO, 3.0);

    let landed = moves
        .iter()
        .position(|(outcome, _)| outcome.grounded)
        .expect("the walker never landed");
    // Free fall from about nine metres takes over a second.
    assert!(
        landed > ticks(1.0),
        "it landed on tick {landed}, not falling"
    );
    assert!(
        moves[landed..].iter().all(|(outcome, _)| outcome.grounded),
        "it did not stay standing once it landed",
    );
    assert_eq!(walker.ground(), Some(Part::Ground));
    let skin = walker.config().skin_width;
    assert!(
        (0.0..=2.0 * skin).contains(&walker.feet().y),
        "it stands with its feet at {}, not on the ground",
        walker.feet().y,
    );
}

/// **A walker walks the path's first leg on the lane**, standing on it every
/// tick — the lane is a slab [`LANE_HEIGHT`] proud of the ground, so the
/// observable is what it stands on and how high — and covers the ground
/// [`WALK_SPEED`] says it should.
#[test]
fn a_walker_walks_the_first_leg_on_the_lane() {
    let map = Map::built_in();
    let start = map.path().waypoints()[0];
    let mut walker = walker();
    standing_at(&mut walker, start + DVec3::Y * LANE_HEIGHT);

    let seconds = 6.0;
    let moves = walk(&mut walker, DVec3::X, seconds);
    assert!(
        moves.iter().all(|(outcome, _)| outcome.grounded),
        "the walker left the ground on the lane",
    );
    assert_eq!(walker.ground(), Some(Part::Lane), "it is not on the lane");
    let feet = walker.feet();
    let skin = walker.config().skin_width;
    assert!(
        (LANE_HEIGHT..=LANE_HEIGHT + 2.0 * skin).contains(&feet.y),
        "its feet are at {}, not on the lane's top",
        feet.y,
    );
    let walked = feet.x - start.x;
    assert!(
        (walked - WALK_SPEED * seconds).abs() < 1e-6,
        "it walked {walked} m in {seconds} s",
    );
    assert!((feet.z - start.z).abs() < 1e-9, "it drifted off the leg");
}

/// **A walker steps up onto a build pad and stands on it**: the pad is
/// [`PAD_HEIGHT`] proud of the ground, well inside the step offset, so it is
/// walked onto rather than walked into.
#[test]
fn a_walker_steps_onto_a_pad_and_stands_on_it() {
    let (_, feet) = plot("entry");
    let mut walker = walker();
    standing_at(&mut walker, feet + DVec3::Z * 3.0);
    let moves = walk(&mut walker, -DVec3::Z, 0.9);

    assert!(
        moves.iter().all(|(outcome, _)| outcome.grounded),
        "the walker left the ground stepping onto the pad",
    );
    assert!(PAD_HEIGHT < walker.config().step_offset);
    assert_eq!(walker.ground(), Some(Part::Pad), "it is not on the pad");
    let skin = walker.config().skin_width;
    assert!(
        (PAD_HEIGHT..=PAD_HEIGHT + 2.0 * skin).contains(&walker.feet().y),
        "its feet are at {}, not on the pad's top",
        walker.feet().y,
    );
}

/// **A tower is solid to the walker, at its tier's size, and gone when it
/// is**: walking straight at the bend plot's tower stops a radius short of
/// it, an upgraded one stops it further out, and an empty plot lets it walk
/// on across.
#[test]
fn a_tower_stops_the_walker_at_its_tiers_size_and_an_empty_plot_does_not() {
    let (at, feet) = plot("bend");
    let radius = walker().config().radius;
    for (tier, scale) in [
        (Tier::Base, 1.0),
        (Tier::Upgraded, f64::from(crate::map::UPGRADED_SCALE)),
    ] {
        let mut walker = walker();
        walker.sync_towers(&one_tower(at, tier));
        standing_at(&mut walker, feet + DVec3::Z * 3.5);
        let moves = walk(&mut walker, -DVec3::Z, 3.0);

        let closest = feet.z + TOWER_RADIUS * scale + radius;
        assert!(
            walker.feet().z >= closest - 1e-6 && walker.feet().z < closest + 0.05,
            "a {tier:?} tower stopped the walker at z {}, not {closest}",
            walker.feet().z,
        );
        assert!(
            moves
                .iter()
                .any(|(_, met)| met.contains(&Some(Part::Tower(at)))),
            "no sweep met the {tier:?} tower",
        );

        walker.sync_towers(&[None; MAX_PLOTS]);
        walk(&mut walker, -DVec3::Z, 1.0);
        assert!(
            walker.feet().z < feet.z,
            "the walker did not cross the plot once its tower was gone",
        );
    }
}

/// **The field's edge holds the walker, as a wall a step cannot climb, and
/// it slides along it**: walked at the edge it stops a radius inside it
/// standing on the slab, and walked at it on a slant it keeps the slant's
/// share along the wall — `CharacterConfig`'s slide rather than a stop.
#[test]
fn the_fields_edge_holds_the_walker_and_it_slides_along_it() {
    let mut walker = walker();
    standing_at(&mut walker, DVec3::new(HALF_WIDTH - 2.0, 0.0, 0.0));
    let radius = walker.config().radius;
    assert!(EDGE_HEIGHT > walker.config().step_offset);

    let moves = walk(&mut walker, DVec3::X, 2.0);
    assert!(
        moves.iter().all(|(outcome, _)| outcome.grounded),
        "the walker left the ground at the edge",
    );
    assert!(
        moves.iter().any(|(_, met)| met.contains(&Some(Part::Edge))),
        "no sweep met the edge",
    );
    let x = walker.feet().x;
    assert!(
        x <= HALF_WIDTH - radius && x > HALF_WIDTH - radius - 0.05,
        "the walker stopped at x {x}, not at the edge",
    );

    let slant = DVec3::new(1.0, 0.0, 1.0).normalize();
    let before = walker.feet();
    walk(&mut walker, slant, 1.0);
    let along = walker.feet().z - before.z;
    assert!(
        along > 0.9 * WALK_SPEED * slant.z,
        "it slid {along} m along the edge, not the slant's share",
    );
    assert!(walker.feet().x <= HALF_WIDTH - radius, "it went through");
    assert!(walker.is_grounded(), "it fell off the field");
}

/// **The exit volume is a trigger in the walker's world as in the stage's**:
/// walked down the last leg the walker passes straight through it, and no
/// sweep ever meets it.
#[test]
fn the_walker_walks_through_the_exit_volume() {
    let map = Map::built_in();
    let exit = map.exit_centre();
    let mut walker = walker();
    standing_at(&mut walker, DVec3::new(exit.x + 3.0, LANE_HEIGHT, exit.z));
    let moves = walk(&mut walker, -DVec3::X, 2.0);

    assert!(
        walker.feet().x < exit.x - crate::map::EXIT_HALF.x - 1.0,
        "the walker stopped at x {}, short of the far side of the exit",
        walker.feet().x,
    );
    assert!(
        moves
            .iter()
            .all(|(_, met)| !met.contains(&Some(Part::Exit))),
        "a sweep met the exit volume",
    );
}
