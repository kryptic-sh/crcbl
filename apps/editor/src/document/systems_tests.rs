//! One entity in several systems: the two-system scene the editor's tests open,
//! attach and detach with their undo, and spawn, delete, duplicate and paste
//! carrying every system's row.

use super::*;

use crcbl::scene_mesh::MESHES;

use super::IN_SCENE;

use crate::scene::{BLOCKS, GREYBOX};

/// The meshes chunk's key, which [`two_systems`] does not hold until a mesh
/// system is listed.
const MESHES_CHUNK: &str = "sys/meshes.ron";

/// The second system [`two_systems`] holds: puppet's sun, which is no thing
/// in space — so an entity in both is placed by its block, and one in the sun
/// alone has no placement.
pub(crate) const SUN: &str = "sun";

/// [`two_systems`]' header: the greybox scene's, listing the sun after the
/// blocks.
const HEADER: &str = "Scene(\n    format: 0,\n    name: \"two\",\n    systems: [\n        \"blocks\",\n        \"sun\",\n    ],\n)";

/// [`two_systems`]' sun chunk: id 1 — the first step, which the blocks chunk
/// also holds — and id 5, which only this chunk spells.
const SUNS: &str = "Chunk(\n    system: \"sun\",\n    entities: [\n        (1, Sun(\n            elevation: 0.5,\n            color: (1.0, 0.9, 0.8),\n            intensity: 2.0,\n            period: 30.0,\n        )),\n        (5, Sun(\n            elevation: -0.25,\n            color: (0.2, 0.3, 0.4),\n            intensity: 1.0,\n            period: 60.0,\n        )),\n    ],\n)";

/// The greybox scene's blocks with a sun chunk beside them: id 1 is a block
/// and a sun, id 5 a sun alone, and ids 0, 2 and 3 blocks alone.
pub(crate) fn two_systems() -> Document {
    let built_in = crate::scene::built_in_source();
    let mut source = MemorySource::new();
    for key in ["env.ron", "sys/blocks.ron"] {
        let text = built_in
            .read(Path::new(&format!("{GREYBOX}/{key}")))
            .expect("the compiled-in scene holds it");
        source
            .insert(Path::new(&format!("two.scn/{key}")), text)
            .expect("a nested scene key is a legal asset key");
    }
    for (key, text) in [("scene.ron", HEADER), ("sys/sun.ron", SUNS)] {
        source
            .insert(
                Path::new(&format!("two.scn/{key}")),
                text.as_bytes().to_vec(),
            )
            .expect("a nested scene key is a legal asset key");
    }
    Document::open(&source, Path::new("two.scn"), crate::scene::vocabulary())
        .expect("one entity in two systems is a scene")
}

/// The first step's id, a block and a sun.
const BOTH: SceneEntityId = SceneEntityId(1);
/// A block alone.
const BLOCK: SceneEntityId = SceneEntityId(2);
/// A sun alone.
const LONE_SUN: SceneEntityId = SceneEntityId(5);

/// A ray down `-Z` through the first step, above the ground slab.
fn ray_at_the_first_step() -> Ray {
    Ray::new(DVec3::new(-3.0, 0.25, 20.0), DVec3::NEG_Z)
}

/// **The two-system scene opens as five entities, the first step in both
/// systems, listed once** — under the first system in manifest order holding
/// it — and writes back byte for byte.
#[test]
fn an_entity_in_two_systems_is_one_entity_listed_once() {
    let mut document = two_systems();
    assert_eq!(document.entity_count(), 5);
    assert_eq!(document.systems_of(BOTH), [BLOCKS, SUN]);
    assert_eq!(document.systems_of(BLOCK), [BLOCKS]);
    assert_eq!(document.systems_of(LONE_SUN), [SUN]);
    assert_eq!(
        document.outline(),
        [
            (BLOCKS.to_owned(), (0..4).map(SceneEntityId).collect()),
            (SUN.to_owned(), vec![LONE_SUN]),
        ],
        "an entity in two systems is not listed once, under its first system",
    );
    assert_eq!(document.placing_system(BOTH).as_deref(), Some(BLOCKS));
    assert_eq!(document.placing_system(LONE_SUN), None);
    assert_eq!(document.attachable(BLOCK), unlisted_after(&[SUN]));
    assert_eq!(document.attachable(BOTH), unlisted_after(&[]));
    assert_eq!(
        document.read(BOTH, SUN, "intensity").expect("its sun"),
        Value::Float(2.0)
    );
    let files = document.files().expect("every entity has an id");
    assert_eq!(files["scene.ron"], HEADER);
    assert_eq!(files["sys/sun.ron"], SUNS);
}

/// **An attach gives an entity its component's `Default` in one more system,
/// and a detach takes one away leaving the rest** — each undone and redone
/// through the log, against the saved text.
#[test]
fn attach_and_detach_undo_and_redo_against_the_saved_text() {
    let mut document = two_systems();
    let before = document.files().expect("ids");

    document.attach(BLOCK, SUN).expect("2 has no sun");
    assert_eq!(document.systems_of(BLOCK), [BLOCKS, SUN]);
    assert_eq!(
        document.read(BLOCK, SUN, "intensity").expect("a sun now"),
        Value::Float(f64::from(crcbl_puppet::map::SUN_INTENSITY)),
        "an attached sun does not start at its type's default",
    );
    let attached = document.files().expect("ids");
    assert!(
        attached["sys/sun.ron"].contains("(2, Sun("),
        "{}",
        attached["sys/sun.ron"]
    );
    assert_eq!(attached["sys/blocks.ron"], before["sys/blocks.ron"]);

    assert_eq!(document.pick(&ray_at_the_first_step()), Some(BOTH));
    document.detach(BOTH, BLOCKS).expect("1 keeps its sun");
    assert_eq!(document.systems_of(BOTH), [SUN]);
    assert_eq!(
        document.bounds(BOTH),
        None,
        "a sun alone is no thing in space"
    );
    assert_eq!(
        document.pick(&ray_at_the_first_step()),
        None,
        "the detached block's collider still picks",
    );
    let detached = document.files().expect("ids");
    assert!(!detached["sys/blocks.ron"].contains("(1, Block("));
    assert_eq!(detached["sys/sun.ron"], attached["sys/sun.ron"]);
    assert_eq!(document.log().len(), 2);

    assert!(document.undo().expect("the detach's inverse applies"));
    assert_eq!(document.files().expect("ids"), attached);
    assert_eq!(document.pick(&ray_at_the_first_step()), Some(BOTH));
    assert!(document.undo().expect("the attach's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
    assert!(!document.is_dirty());

    assert!(document.redo().expect("the attach applies again"));
    assert_eq!(document.files().expect("ids"), attached);
    assert!(document.redo().expect("the detach applies again"));
    assert_eq!(document.files().expect("ids"), detached);
}

/// `first`, then every system the vocabulary registers that [`two_systems`]'
/// manifest does not list, by group label and then by name, the ungrouped
/// last: what an entity holding every listed system but `first` could be
/// attached to.
fn unlisted_after(first: &[&str]) -> Vec<String> {
    let vocabulary = crate::scene::vocabulary();
    let mut unlisted: Vec<&str> = vocabulary
        .systems()
        .filter(|system| ![BLOCKS, SUN].contains(system))
        .collect();
    unlisted.sort_by_key(|system| {
        let group = vocabulary.group_of(system);
        (group.is_none(), group, *system)
    });
    first
        .iter()
        .copied()
        .chain(unlisted)
        .map(str::to_owned)
        .collect()
}

/// **The systems an entity could join are grouped by the game that
/// registered them, the scene's own first**: a block in the two-system scene
/// is offered the sun under "In this scene", then each game's systems under
/// its label in label order, and no heading whose systems it holds already.
#[test]
fn attachable_systems_are_grouped_by_game_with_the_scenes_first() {
    let mut document = two_systems();
    let groups: Vec<(String, Vec<String>)> = document
        .attachable_groups(BLOCK)
        .into_iter()
        .map(|group| (group.label, group.systems))
        .collect();
    let expected: Vec<(String, Vec<String>)> = [
        (IN_SCENE, &[SUN][..]),
        ("breakout", &["bricks"]),
        ("engine", &["bodies", MESHES]),
        ("puppet", &["spawn", "surfaces"]),
        ("towers", &["plots", "waypoints"]),
    ]
    .into_iter()
    .map(|(label, systems)| {
        (
            label.to_owned(),
            systems.iter().map(|system| (*system).to_owned()).collect(),
        )
    })
    .collect();
    assert_eq!(groups, expected, "the greybox block's own group is held");

    let flat: Vec<String> = expected
        .into_iter()
        .flat_map(|(_, systems)| systems)
        .collect();
    assert_eq!(document.attachable(BLOCK), flat);
    let both: Vec<String> = document
        .attachable_groups(BOTH)
        .into_iter()
        .map(|group| group.label)
        .collect();
    assert_eq!(
        both,
        ["breakout", "engine", "puppet", "towers"],
        "a heading with nothing left to offer is drawn",
    );
}

/// **Attach offers every registered system, and attaching to one the manifest
/// does not list lists it at the end in the same entry** — so the row is
/// saved, and one undo puts every file back byte for byte, the manifest and
/// the chunk the listing added included.
#[test]
fn attaching_to_an_unlisted_system_lists_it_and_one_undo_puts_the_files_back() {
    let mut document = two_systems();
    let before = document.files().expect("ids");
    assert!(!before.contains_key(MESHES_CHUNK));
    let offered = document.attachable(BLOCK);
    assert_eq!(offered[0], SUN, "the manifest's systems come first");
    assert!(offered.contains(&MESHES.to_owned()), "{offered:?}");

    document.attach(BLOCK, MESHES).expect("2 has no mesh");
    assert_eq!(document.systems_of(BLOCK), [BLOCKS, MESHES]);
    assert_eq!(
        document.log().len(),
        1,
        "the listing and the attach are one"
    );
    let attached = document.files().expect("ids");
    assert_eq!(
        attached["scene.ron"],
        HEADER.replace("\"sun\",\n", "\"sun\",\n        \"meshes\",\n"),
    );
    assert!(
        attached[MESHES_CHUNK].contains("(2, Mesh("),
        "{}",
        attached[MESHES_CHUNK]
    );
    assert!(!document.attachable(BLOCK).contains(&MESHES.to_owned()));

    assert!(document.undo().expect("the batch's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
    assert!(!document.is_dirty());
    assert!(document.redo().expect("the batch applies again"));
    assert_eq!(document.files().expect("ids"), attached);
}

/// **Detaching the last entity of a system leaves the system listed**: its
/// chunk is saved empty, and unlisting it is an edit of its own.
#[test]
fn detaching_a_systems_last_entity_leaves_it_listed() {
    let mut document = two_systems();
    document.attach(BLOCK, MESHES).expect("2 has no mesh");
    let attached = document.files().expect("ids");
    document.detach(BLOCK, MESHES).expect("2 keeps its block");
    let detached = document.files().expect("ids");
    assert_eq!(detached["scene.ron"], attached["scene.ron"]);
    assert!(
        !detached[MESHES_CHUNK].contains("Mesh("),
        "{}",
        detached[MESHES_CHUNK]
    );
    assert_eq!(document.log().len(), 2);
}

/// **An attach or a detach the scene cannot take is refused by its reason,
/// and nothing is recorded**: a detach of the last system, of a system that
/// does not hold the entity, an attach to one that does or to one the
/// vocabulary does not register, an absent entity, and a row that is not the
/// component.
#[test]
fn an_attach_or_detach_the_scene_cannot_take_is_refused() {
    let mut document = two_systems();
    let before = document.files().expect("ids");

    let error = document.detach(BLOCK, BLOCKS).expect_err("its last system");
    assert!(
        matches!(error, EditError::NoComponent(id) if id == BLOCK),
        "{error}"
    );
    let error = document.detach(BLOCK, SUN).expect_err("2 has no sun");
    assert!(
        matches!(&error, EditError::NotAttached { entity, system } if *entity == BLOCK && system == SUN),
        "{error}"
    );
    let error = document.attach(BOTH, SUN).expect_err("1 has a sun");
    assert!(
        matches!(&error, EditError::Attached { entity, system } if *entity == BOTH && system == SUN),
        "{error}"
    );
    let error = document
        .attach(BLOCK, "nonsense")
        .expect_err("not in the vocabulary");
    assert!(
        matches!(&error, EditError::NoSystem(system) if system == "nonsense"),
        "{error}"
    );
    let error = document
        .attach(SceneEntityId(9_999), SUN)
        .expect_err("no such entity");
    assert!(matches!(error, EditError::NoEntity(_)), "{error}");
    let error = document
        .apply(EditCommand::Attach {
            entity: BLOCK,
            system: SUN.to_owned(),
            row: "(intensity:1.0)".to_owned(),
        })
        .expect_err("a sun has an elevation");
    assert!(matches!(error, EditError::Scene(_)), "{error}");

    assert!(document.log().is_empty(), "a refused edit was recorded");
    assert_eq!(document.systems_of(BLOCK), [BLOCKS]);
    assert_eq!(document.files().expect("ids"), before);
}

/// **A delete takes every system's row and its undo brings each back**, under
/// the old id, byte for byte.
#[test]
fn a_delete_of_an_entity_in_two_systems_is_undone_in_both() {
    let mut document = two_systems();
    let before = document.files().expect("ids");
    document.delete(&[BOTH]).expect("held");
    let deleted = document.files().expect("ids");
    assert!(!deleted["sys/blocks.ron"].contains("(1, Block("));
    assert!(!deleted["sys/sun.ron"].contains("(1, Sun("));

    assert!(document.undo().expect("the spawn of both rows applies"));
    assert_eq!(document.files().expect("ids"), before);
    assert_eq!(document.systems_of(BOTH), [BLOCKS, SUN]);
}

/// **A duplicate and a paste carry every system's row**: the copy is a block
/// and a sun, each the original's to the bit.
#[test]
fn a_duplicate_and_a_paste_carry_every_systems_row() {
    let mut document = two_systems();
    let copy = document.duplicate(&[BOTH]).expect("held")[0];
    assert_eq!(document.systems_of(copy), [BLOCKS, SUN]);
    for (system, path) in [(BLOCKS, "position.0"), (SUN, "period")] {
        assert_eq!(
            document.read(copy, system, path).expect("copied"),
            document.read(BOTH, system, path).expect("held"),
            "the copy's {system} {path} is not the original's",
        );
    }

    let text = document.copy(&[BOTH]).expect("held");
    let pasted = document.paste(&text).expect("a clipping of this scene");
    assert_eq!(pasted.len(), 1);
    assert_eq!(document.systems_of(pasted[0]), [BLOCKS, SUN]);
    assert_eq!(
        document.read(pasted[0], SUN, "elevation").expect("pasted"),
        document.read(BOTH, SUN, "elevation").expect("held"),
    );
}

/// **A clipping written before entities could span systems still pastes**,
/// as an entity in the one system it names.
#[test]
fn a_clipping_from_before_several_systems_still_pastes() {
    let mut document = two_systems();
    let older = "Entities(\n    entities: [\n        Entity(\n            system: \"blocks\",\n            row: \"(position:(9.0,0.5,0.0),half_extents:(0.5,0.5,0.5))\",\n        ),\n    ],\n)";
    let pasted = document.paste(older).expect("an older clipping");
    assert_eq!(pasted.len(), 1);
    assert_eq!(document.systems_of(pasted[0]), [BLOCKS]);
    assert_eq!(
        document
            .read(pasted[0], BLOCKS, "position.0")
            .expect("a block"),
        Value::Float(9.0)
    );
}

/// **A spawn naming no system, or one system twice, is refused**: an entity
/// in no system is in no file, and a second row would replace the first.
#[test]
fn a_spawn_of_no_rows_or_a_repeated_system_is_refused() {
    let mut document = two_systems();
    let id = SceneEntityId(9);
    let error = document
        .apply(EditCommand::Spawn {
            entity: id,
            rows: Vec::new(),
            name: None,
        })
        .expect_err("no rows");
    assert!(matches!(error, EditError::NoComponent(_)), "{error}");

    let row = document.rows(BLOCK).expect("held").remove(0);
    let error = document
        .apply(EditCommand::Spawn {
            entity: id,
            rows: vec![row.clone(), row],
            name: None,
        })
        .expect_err("blocks twice");
    assert!(
        matches!(&error, EditError::Attached { system, .. } if system == BLOCKS),
        "{error}"
    );
    assert_eq!(document.entity_count(), 5);
    assert!(document.log().is_empty());
}

/// **Attach and detach are refused in play mode**, and leave the played scene
/// and the log as they were.
#[test]
fn attach_and_detach_are_refused_in_play() {
    let mut document = two_systems();
    document.play().expect("a scene with no module still plays");
    let error = document.attach(BLOCK, SUN).expect_err("playing");
    assert!(matches!(error, EditError::Playing), "{error}");
    let error = document.detach(BOTH, SUN).expect_err("playing");
    assert!(matches!(error, EditError::Playing), "{error}");
    assert!(document.log().is_empty());
    document.stop().expect("stops");
    assert_eq!(document.systems_of(BLOCK), [BLOCKS]);
    assert_eq!(document.systems_of(BOTH), [BLOCKS, SUN]);
}

/// **A new entity in one system, from a scene with nothing in it**: the next
/// id, holding that system's `Default` and nothing else, the system listed in
/// front of it — one undo takes the entity and the
/// listing back to the empty scene's files, and a second entity in a listed
/// system lists nothing. Refused in play and for a system the vocabulary does
/// not hold, recording nothing.
#[test]
fn add_entity_puts_a_default_component_in_one_system_listing_it_first() {
    /// Towers' path, which no scene from empty lists.
    const WAYPOINTS: &str = "waypoints";

    let mut document = crate::scene::built_in_document().expect("the compiled-in scene opens");
    document.new_scene().expect("editing");
    let empty = document.files().expect("ids");
    document.play().expect("a scene of nothing still plays");
    let error = document.add_entity(WAYPOINTS).expect_err("playing");
    assert!(matches!(error, EditError::Playing), "{error}");
    document.stop().expect("stops");
    assert!(document.log().is_empty(), "a refusal recorded");
    assert_eq!(document.files().expect("ids"), empty);

    let first = document
        .add_entity(WAYPOINTS)
        .expect("towers' path is registered");
    assert_eq!(first, SceneEntityId(0));
    assert_eq!(document.systems_of(first), [WAYPOINTS]);
    assert_eq!(document.manifest(), [WAYPOINTS]);
    assert_eq!(
        document
            .component(first, WAYPOINTS)
            .and_then(|row| row.as_any().downcast_ref::<crcbl_towers::Waypoint>())
            .copied(),
        Some(crcbl_towers::Waypoint::default()),
        "the new waypoint is not its type's default",
    );
    assert_eq!(document.log().len(), 1, "an add is one entry");
    let added = document.files().expect("ids");

    let second = document.add_entity(WAYPOINTS).expect("listed now");
    assert_eq!(second, SceneEntityId(1));
    assert!(
        matches!(
            document.log().applied().last(),
            Some(EditCommand::Spawn { .. })
        ),
        "an add in a listed system listed it again",
    );

    assert!(document.undo().expect("applies"));
    assert_eq!(document.files().expect("ids"), added);
    assert!(document.undo().expect("applies"));
    assert_eq!(document.files().expect("ids"), empty, "the listing stayed");
    assert!(document.redo().expect("applies"));
    assert_eq!(document.files().expect("ids"), added);

    let error = document
        .add_entity("no-such-system")
        .expect_err("not registered");
    assert!(matches!(error, EditError::NoSystem(_)), "{error}");
    assert_eq!(document.files().expect("ids"), added);
    assert_eq!(document.log().position(), 1, "a refusal recorded");
}
