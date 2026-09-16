//! **The editor opens a scene it did not write**, for two games' vocabularies
//! and neither of them this build's own.
//!
//! `crcbl_editor` names no game — `src/scene.rs` is the vocabulary the binary
//! ships with, and `src/document.rs` and `src/app.rs` ask a
//! [`Registry`](crcbl::registry::Registry) and nothing else. That is a claim
//! about a seam, and a seam exercised by one vocabulary is a seam that has not
//! been exercised: this file is the second and the third.
//!
//! Both samples are `[dev-dependencies]`, so they reach these test binaries and
//! no shipped one. What each contributes:
//!
//! * `apps/breakout` — one system, `bricks`, whose component is the one slice 1
//!   hard-coded. The placement here has to agree with what that slice read off
//!   `Brick::position` and `Brick::half_extents` by hand.
//! * `apps/puppet` — **three** systems, one of which (`sun`) is not a thing in
//!   space, so a load through the registry has to bring back rows an outliner
//!   lists and a ray cannot hit.

use std::path::Path;

use crcbl::assets::AssetSource;
use crcbl::reflect::Value;
use crcbl::registry::{Placement, Registry};
use crcbl::scene::scn::{SceneEntityId, ScnError};
use crcbl_editor::{Document, EditCommand, EditError};

/// `apps/breakout`'s vocabulary, registered by the game itself.
fn breakout_registry() -> Registry {
    let mut registry = Registry::new();
    crcbl_breakout::register_components(&mut registry);
    registry
}

/// `apps/puppet`'s vocabulary, registered by the sample itself.
fn puppet_registry() -> Registry {
    let mut registry = Registry::new();
    crcbl_puppet::map::register_components(&mut registry);
    registry
}

/// The editor's own document over breakout's committed board.
fn breakout_document() -> Document {
    Document::open(
        &crcbl_breakout::built_in_source(),
        Path::new(crcbl_breakout::BOARD),
        breakout_registry(),
    )
    .expect("the committed board is a scene")
}

/// The editor's own document over puppet's committed blockout.
fn puppet_document() -> Document {
    Document::open(
        &crcbl_puppet::map::built_in_source(),
        Path::new(crcbl_puppet::map::BLOCKOUT),
        puppet_registry(),
    )
    .expect("the committed blockout is a scene")
}

/// **The editor opens breakout's board**, with every brick the file names and
/// under the system name the manifest spells — through the game's own
/// registration and no vocabulary of the editor's.
#[test]
fn the_editor_opens_breakouts_board_through_the_games_own_registration() {
    let mut document = breakout_document();
    assert_eq!(document.name(), "board");

    let outline = document.outline();
    assert_eq!(outline.len(), 1, "the manifest names one system");
    assert_eq!(outline[0].0, "bricks");
    assert_eq!(
        outline[0].1.len(),
        crcbl_breakout::Board::built_in().bricks().len(),
        "the editor and the game read a different number of bricks out of one file",
    );
    assert_eq!(document.entity_count(), outline[0].1.len());
    assert!(!document.is_dirty(), "a document nobody edited opens clean");
}

/// **The editor opens puppet's map**, whose manifest names three systems — the
/// claim a registry with one entry could not make.
#[test]
fn the_editor_opens_puppets_map_and_all_three_of_its_systems() {
    let mut document = puppet_document();
    assert_eq!(document.name(), "blockout");

    let outline = document.outline();
    let systems: Vec<&str> = outline.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        systems,
        ["surfaces", "spawn", "sun"],
        "the outline is not the manifest's own order",
    );
    assert_eq!(
        outline[0].1.len(),
        crcbl_puppet::map::Map::built_in().surfaces().len(),
        "the editor and the sample read a different number of surfaces out of one file",
    );
    assert_eq!(outline[1].1, [SceneEntityId(5)], "one spawn, id 5");
    assert_eq!(outline[2].1, [SceneEntityId(6)], "one sun, id 6");
    assert_eq!(document.entity_count(), 7);
}

/// **The placement the registry answers is what slice 1 hard-coded**: a brick's
/// own centre and half extents, read off the component rather than off a field
/// name a tool knew.
///
/// Against the game's parse of the same file rather than against the registry's
/// other half, so a placement that read the wrong field is red here.
#[test]
fn a_bricks_placement_is_the_one_slice_one_read_by_hand() {
    let mut document = breakout_document();
    let board = crcbl_breakout::Board::built_in();

    for (index, brick) in board.bricks().iter().enumerate() {
        let id = SceneEntityId(u32::try_from(index).expect("a board of fewer than 2^32 bricks"));
        let (min, max) = document
            .bounds(id)
            .unwrap_or_else(|| panic!("brick {index} has a placement"));
        let centre = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        for axis in 0..3 {
            assert!(
                (f64::from(centre[axis]) - brick.position[axis]).abs() < 1e-4,
                "brick {index} centre axis {axis}: {centre:?} against {:?}",
                brick.position,
            );
            assert!(
                (f64::from(half[axis]) - brick.half_extents[axis]).abs() < 1e-4,
                "brick {index} half extent axis {axis}: {half:?} against {:?}",
                brick.half_extents,
            );
        }
    }

    // And the trait the registry reaches it through answers the same thing
    // directly, which is what says the accessor is the component's and not a
    // copy of it.
    let first = board.bricks()[0];
    assert_eq!(
        first.placement(),
        Some((first.position(), first.half_extents())),
    );
}

/// **A surface's placement is the collider the sample already builds.** The
/// platform arm is the one both halves read, so a tool draws the box the
/// character actually walks into.
#[test]
fn a_surfaces_placement_is_the_box_puppet_collides_with() {
    let mut document = puppet_document();
    let map = crcbl_puppet::map::Map::built_in();

    let ground = map.surfaces()[0].clone();
    assert_eq!(
        ground.label, "ground",
        "the fixture assumes row 0 is ground"
    );
    let crcbl_puppet::map::Shape::Platform { width, height, .. } = ground.shape else {
        panic!("the ground is a platform");
    };

    let (min, max) = document.bounds(SceneEntityId(0)).expect("the ground");
    let centre = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    assert!(
        (f64::from(centre.y) - (ground.position[1] + 0.5 * height)).abs() < 1e-4,
        "a platform stands on its origin, so its centre is half a height up: {centre:?}",
    );
    assert!((f64::from(half.x) - 0.5 * width).abs() < 1e-4, "{half:?}");
}

/// **A row that is not a thing in space has no bounds and is still editable.**
/// `Sun` is a direction, a colour and a rate: an outliner lists it, a property
/// path reaches it, and a ray cannot hit it.
#[test]
fn puppets_sun_is_editable_and_has_no_bounds() {
    let mut document = puppet_document();
    let sun = SceneEntityId(6);

    assert_eq!(
        document.bounds(sun),
        None,
        "a sun drawn as a box is a pickable cube in the middle of every map",
    );
    assert_eq!(
        document
            .read(sun, "intensity")
            .expect("the sun's own row is reachable through the registry"),
        Value::Float(f64::from(2.2_f32)),
    );
}

/// **A scene whose manifest names an unregistered system is refused by that
/// system's name**, rather than opened with the chunk missing.
///
/// Puppet's map through breakout's vocabulary: every one of its three systems is
/// unknown, and the first is the one the refusal has to name.
#[test]
fn a_scene_naming_an_unregistered_system_is_refused_by_name() {
    let error = Document::open(
        &crcbl_puppet::map::built_in_source(),
        Path::new(crcbl_puppet::map::BLOCKOUT),
        breakout_registry(),
    )
    .expect_err("breakout's vocabulary has no `surfaces`");

    let EditError::Scene(ScnError::NoCodec { system }) = &error else {
        panic!("the refusal is not about a missing codec: {error}");
    };
    assert_eq!(system, "surfaces");
    assert!(
        error.to_string().contains("surfaces"),
        "the message does not name the system: {error}",
    );
}

/// **Both games in one registry, and one editor opens both scenes.** The
/// vocabularies compose: registering a second game adds names rather than
/// replacing the first's.
#[test]
fn one_registry_holding_both_games_opens_either_scene() {
    let both = || {
        let mut registry = Registry::new();
        crcbl_breakout::register_components(&mut registry);
        crcbl_puppet::map::register_components(&mut registry);
        registry
    };
    assert_eq!(
        both().systems().collect::<Vec<_>>(),
        ["bricks", "spawn", "sun", "surfaces"],
        "the vocabularies did not compose",
    );

    let board = Document::open(
        &crcbl_breakout::built_in_source(),
        Path::new(crcbl_breakout::BOARD),
        both(),
    )
    .expect("a registry holding bricks opens the board");
    let blockout = Document::open(
        &crcbl_puppet::map::built_in_source(),
        Path::new(crcbl_puppet::map::BLOCKOUT),
        both(),
    )
    .expect("the same registry opens the blockout");

    assert_eq!(board.name(), "board");
    assert_eq!(blockout.name(), "blockout");
    assert_eq!(blockout.entity_count(), 7);
    assert_eq!(
        board.entity_count(),
        crcbl_breakout::Board::built_in().bricks().len(),
        "a registry that knows two games read a different board from one that knows one",
    );
}

/// **Load → save with no edits is byte-identical, for both games' committed
/// scenes.**
///
/// Against the committed files themselves rather than against a second save: a
/// writer that was merely self-consistent would pass that, and the claim is that
/// opening either of these scenes in the editor and saving it changes nothing on
/// disk.
#[test]
fn both_games_committed_scenes_round_trip_byte_for_byte() {
    for (name, mut document, source, dir, keys) in [
        (
            "breakout",
            breakout_document(),
            crcbl_breakout::built_in_source(),
            crcbl_breakout::BOARD,
            vec!["env.ron", "scene.ron", "sys/bricks.ron"],
        ),
        (
            "puppet",
            puppet_document(),
            crcbl_puppet::map::built_in_source(),
            crcbl_puppet::map::BLOCKOUT,
            vec![
                "env.ron",
                "scene.ron",
                "sys/spawn.ron",
                "sys/sun.ron",
                "sys/surfaces.ron",
            ],
        ),
    ] {
        let written = document.files().expect("every row has an id");
        assert_eq!(
            written.keys().collect::<Vec<_>>(),
            keys.iter().collect::<Vec<_>>(),
            "{name}: the save wrote a different set of files",
        );
        for (key, text) in &written {
            let committed = source
                .read(Path::new(&format!("{dir}/{key}")))
                .unwrap_or_else(|error| panic!("{name}: the committed {key}: {error}"));
            assert_eq!(
                text.as_bytes(),
                committed.as_slice(),
                "{name}: a load-save round trip changed {key}",
            );
        }
    }
}

/// **An edit through the editor's own command path reaches a game's component**,
/// and the file that comes back holds exactly the edited value.
///
/// Puppet's, because its component is the one in the workspace with a `String`,
/// an enum and two arrays — a path resolution that only ever worked on a flat
/// struct of floats passes every test built on breakout's `Brick`.
#[test]
fn an_edit_reaches_a_nested_field_of_puppets_surface() {
    let mut document = puppet_document();
    // Row 3 is the gentle mound, whose radius the sample's own constant is what
    // the committed file was written from.
    let gentle = SceneEntityId(3);
    let before = document
        .read(gentle, "shape.radius")
        .expect("a dome has a radius");
    assert_eq!(before, Value::Float(crcbl_puppet::map::GENTLE_MOUND.2));

    document
        .apply(EditCommand::SetProperty {
            entity: gentle,
            path: "shape.radius".to_owned(),
            value: Value::Float(7.5),
        })
        .expect("a dome's radius is a leaf");
    assert_eq!(
        document
            .read(gentle, "shape.radius")
            .expect("the edited leaf is still there"),
        Value::Float(7.5),
    );

    // The bounds a selection is drawn with are read back off the component, so
    // an edit to a field *inside the enum variant* changes the box — which is
    // the half a placement that only ever read a top-level `position` would get
    // wrong. (That the *collider* is rebuilt too is a pick, and
    // `crcbl_editor::document`'s `a_moved_block_is_picked_where_it_now_is` is
    // what observes it.)
    let (min, max) = document.bounds(gentle).expect("a dome is a thing in space");
    let half = (max - min) * 0.5;
    assert!(
        (f64::from(half.x) - 7.5).abs() < 1e-4,
        "the bounds do not read the edited radius: {half:?}",
    );

    assert!(document.undo().expect("one entry"));
    assert_eq!(
        document
            .read(gentle, "shape.radius")
            .expect("and after the undo"),
        before,
    );
    assert!(!document.is_dirty(), "undoing back to the load is clean");
}

/// The bounds a spawn point is drawn with are the character's own capsule, not a
/// marker of some size picked to look right.
#[test]
fn a_spawn_points_bounds_are_the_character_it_will_hold() {
    let mut document = puppet_document();
    let (min, max) = document.bounds(SceneEntityId(5)).expect("the spawn");
    let centre = (min + max) * 0.5;
    let half = (max - min) * 0.5;

    let feet = crcbl_puppet::map::SPAWN;
    let half_height = 0.5 * crcbl_puppet::map::CHARACTER_HEIGHT;
    assert!(
        (f64::from(centre.y) - (feet.y + half_height)).abs() < 1e-4,
        "{centre:?}"
    );
    assert!((f64::from(centre.z) - feet.z).abs() < 1e-4, "{centre:?}");
    assert!((f64::from(half.y) - half_height).abs() < 1e-4, "{half:?}");
    assert!(
        (f64::from(half.x) - crcbl_puppet::map::CHARACTER_RADIUS).abs() < 1e-4,
        "{half:?}",
    );
}
