//! The document's own tests, over a vocabulary of one test component: the
//! cases that reach inside it. The editor's suite (`apps/editor/src/document/`)
//! opens it through the shipped vocabulary — the games' components and the
//! editor's greybox block, which this crate cannot name.

use std::hash::Hasher;

use serde::{Deserialize, Serialize};

use super::*;
use crate::ecs::ComponentHash;
use crate::registry::{Placement, Validate};

/// The one system [`vocabulary`] registers.
pub(super) const BLOCKS: &str = "blocks";

/// A box: where it stands and how far it reaches, with no rule of its own —
/// so a value written past the commands is not refused by one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
struct Block {
    position: [f64; 3],
    half_extents: [f64; 3],
}

impl ComponentHash for Block {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        for value in self.position.iter().chain(&self.half_extents) {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

impl Placement for Block {
    fn placement(&self) -> Option<OrientedBox> {
        Some(OrientedBox::axis_aligned(
            DVec3::from_array(self.position),
            DVec3::from_array(self.half_extents),
        ))
    }
}

impl Validate for Block {}

/// [`Block`], under [`BLOCKS`].
pub(super) fn vocabulary() -> Registry {
    let mut registry = Registry::new();
    registry.register::<Block>(BLOCKS);
    registry
}

/// A scene of one unit block at the origin, keyed at the source's root.
pub(super) fn one_block() -> Document {
    let mut source = empty_source();
    for (key, text) in [
        (
            "scene.ron",
            "Scene(\n    format: 0,\n    name: \"one\",\n    systems: [\n        \"blocks\",\n    ],\n)",
        ),
        (
            "sys/blocks.ron",
            "Chunk(\n    system: \"blocks\",\n    entities: [\n        (0, Block(\n            \
             position: (0.0, 0.0, 0.0),\n            half_extents: (1.0, 1.0, 1.0),\n        \
             )),\n    ],\n)",
        ),
    ] {
        source
            .insert(Path::new(key), text.as_bytes().to_vec())
            .expect("a scene key is a legal asset key");
    }
    Document::open(&source, Path::new(""), vocabulary()).expect("one block is a scene")
}

/// A ray down `-Z` through the block's centre.
fn through_the_block() -> Ray {
    Ray::new(DVec3::new(0.0, 0.0, 20.0), DVec3::NEG_Z)
}

/// **A scene reads its assets from its game's root**, the nearest
/// directory above it with a manifest, and from the directory holding it
/// outside any project.
#[test]
fn a_scenes_asset_root_is_its_games_root() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let game = dir.path().join("game");
    let scenes = game.join("levels").join("one");
    std::fs::create_dir_all(&scenes).expect("the scene folders");
    let scene = scenes.join("field.scn");

    assert_eq!(asset_root(&scene), scenes, "outside a project");

    std::fs::write(game.join(PROJECT_MARKER), "").expect("a manifest");
    assert_eq!(asset_root(&scene), game, "inside a project");
}

/// **A box no collider can be is left unpickable, not a crash.** A value
/// written past the commands — here a block's half extent below zero —
/// reaches the picking sync without any rule seeing it; the sync must take
/// the entity's collider away rather than hand the physics a box it
/// refuses.
#[test]
fn a_box_no_collider_can_be_is_left_unpickable() {
    let mut document = one_block();
    let id = document.pick(&through_the_block()).expect("the block");
    let block = document.component(id, BLOCKS).expect("a block");
    set_path(block, "half_extents.0", &Value::Float(-1.0)).expect("a block's leaf");

    let entity = document.ids.entity(id).expect("a filed id");
    sync_colliders(&document.registry, &mut document.world, [entity]);
    assert_eq!(
        document.pick(&through_the_block()),
        None,
        "the bad box still picks"
    );

    let block = document.component(id, BLOCKS).expect("a block");
    set_path(block, "half_extents.0", &Value::Float(1.0)).expect("a block's leaf");
    sync_colliders(&document.registry, &mut document.world, [entity]);
    assert_eq!(
        document.pick(&through_the_block()),
        Some(id),
        "a fixed box picks again"
    );
}

/// **A spawn with its fields is one entry**, its fields read as their leaves'
/// kinds, and one undo takes it all back; a field refused spawns nothing.
#[test]
fn a_spawn_with_fields_is_one_entry_and_a_refused_field_spawns_nothing() {
    let mut document = one_block();
    let before = document.files().expect("ids");

    let refused = document.spawn_with(BLOCKS, &[("position.0", "2.5"), ("height", "1.0")]);
    assert!(matches!(refused, Err(EditError::Path(_))), "{refused:?}");
    assert_eq!(
        document.files().expect("ids"),
        before,
        "a refused spawn left a row"
    );
    assert!(document.log().is_empty(), "a refused spawn was recorded");

    let id = document
        .spawn_with(BLOCKS, &[("position.0", "2.5"), ("half_extents.1", "0.25")])
        .expect("a block spawns");
    assert_eq!(
        document.log().len(),
        1,
        "the fields took entries of their own"
    );
    assert_eq!(
        document.read(id, BLOCKS, "position.0").ok(),
        Some(Value::Float(2.5))
    );
    assert_eq!(
        document.read(id, BLOCKS, "half_extents.1").ok(),
        Some(Value::Float(0.25))
    );
    assert!(document.undo().expect("the inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
}

/// **A component's fields are listed as the chunk file spells them**, every
/// leaf by its path, in declaration order.
#[test]
fn a_components_fields_are_listed_by_path_as_the_file_spells_them() {
    let mut document = one_block();
    let texts = document
        .field_texts(SceneEntityId(0), BLOCKS)
        .expect("the block");
    let expected = [
        ("position.0", "0.0"),
        ("position.1", "0.0"),
        ("position.2", "0.0"),
        ("half_extents.0", "1.0"),
        ("half_extents.1", "1.0"),
        ("half_extents.2", "1.0"),
    ];
    assert_eq!(
        texts,
        expected
            .iter()
            .map(|&(path, text)| (path.to_owned(), text.to_owned()))
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        document.field_texts(SceneEntityId(9), BLOCKS),
        Err(EditError::NoEntity(_))
    ));
}
