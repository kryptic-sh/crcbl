//! Spawn, delete and duplicate: the edits that change which entities a
//! document holds. The property test that walks random histories of them —
//! and of every other edit — back is `document::undo_property_tests`.

use super::*;

use crate::command::SystemRow;

fn document() -> Document {
    crate::scene::built_in_document().expect("the compiled-in scene is a scene")
}

/// A ray down `-Z` through the first step, and through nothing else: the
/// ground slab's top is `y = 0` and this is above it.
fn ray_at_the_first_step() -> Ray {
    Ray::new(DVec3::new(-3.0, 0.25, 20.0), DVec3::NEG_Z)
}

/// Every id the document holds, in outline order.
fn ids(document: &mut Document) -> Vec<SceneEntityId> {
    document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .collect()
}

/// **A deleted entity is gone from the outline, the pick and the save, and an
/// undo brings all three back** — the file byte for byte, under its old id.
#[test]
fn a_delete_removes_the_entity_everywhere_and_an_undo_restores_it() {
    let mut document = document();
    let before = document.files().expect("every block has an id");
    let first = SceneEntityId(1);
    assert_eq!(document.pick(&ray_at_the_first_step()), Some(first));

    document
        .delete(&[first])
        .expect("the first step is in the scene");
    assert!(!ids(&mut document).contains(&first));
    assert_eq!(document.entity_count(), 3);
    assert_eq!(
        document.pick(&ray_at_the_first_step()),
        None,
        "a deleted entity's collider is still in the physics world",
    );
    let files = document.files().expect("ids");
    assert!(
        !files["sys/blocks.ron"].contains("(1, Block("),
        "the save still writes the deleted row:\n{}",
        files["sys/blocks.ron"],
    );
    assert!(document.is_dirty());

    assert!(document.undo().expect("the delete's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
    assert_eq!(document.pick(&ray_at_the_first_step()), Some(first));
    assert!(!document.is_dirty());
}

/// **A duplicate is the original's component under the next id**, and its undo
/// and redo bring it back under that same id rather than a new one.
#[test]
fn a_duplicate_copies_the_row_under_the_next_id() {
    let mut document = document();
    let before = document.files().expect("ids");
    let copy = document
        .duplicate(&[SceneEntityId(3)])
        .expect("the third step is in the scene")[0];
    assert_eq!(copy, SceneEntityId(4), "one past the file's highest id");
    assert_eq!(document.entity_count(), 5);
    for field in ["position", "half_extents"] {
        for axis in 0..3 {
            let path = format!("{field}.{axis}");
            assert_eq!(
                document
                    .read(copy, crate::scene::BLOCKS, &path)
                    .expect("the copy is a block"),
                document
                    .read(SceneEntityId(3), crate::scene::BLOCKS, &path)
                    .expect("a block"),
                "the copy's {path} is not the original's",
            );
        }
    }
    let duplicated = document.files().expect("ids");
    assert!(
        duplicated["sys/blocks.ron"]
            .contains("(4, Block(\n            position: (3.0, 1.25, 0.0),"),
        "{}",
        duplicated["sys/blocks.ron"],
    );

    assert!(document.undo().expect("the spawn's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
    assert!(document.redo().expect("the spawn applies again"));
    assert_eq!(document.files().expect("ids"), duplicated);
}

/// **An edit recorded against an entity survives that entity being deleted and
/// restored**, because the restore files it under the id the edit names.
#[test]
fn an_edit_to_a_restored_entity_finds_it_by_its_old_id() {
    let mut document = document();
    let first = SceneEntityId(1);
    document
        .apply(EditCommand::SetProperty {
            entity: first,
            system: crate::scene::BLOCKS.to_owned(),
            path: "position.0".to_owned(),
            value: Value::Float(-4.0),
        })
        .expect("a block has an x");
    document
        .delete(&[first])
        .expect("the first step is in the scene");
    document.undo().expect("restore it");
    document.undo().expect("and walk the move back");
    assert_eq!(
        document
            .read(first, crate::scene::BLOCKS, "position.0")
            .expect("restored"),
        Value::Float(-3.0),
    );
}

/// Deleting the selected entity clears the selection, so nothing points at a
/// hole.
#[test]
fn deleting_the_selection_clears_it() {
    let mut document = document();
    document.select(Some(SceneEntityId(2)));
    document.delete(&[SceneEntityId(2)]).expect("in the scene");
    assert_eq!(document.primary(), None);
}

/// **A spawn that would break the scene is refused, and nothing is recorded or
/// left behind**: an id in use, a system the manifest does not list, and a row
/// that is not the system's component.
#[test]
fn a_spawn_the_scene_cannot_hold_is_refused_and_leaves_nothing() {
    let mut document = document();
    let before = document.files().expect("ids");
    let row = "(position:(0.0,0.0,0.0),half_extents:(1.0,1.0,1.0))".to_owned();

    let error = document
        .apply(EditCommand::Spawn {
            entity: SceneEntityId(2),
            rows: vec![SystemRow {
                system: crate::scene::BLOCKS.to_owned(),
                row: row.clone(),
            }],
            name: None,
        })
        .expect_err("2 is the second step's");
    assert!(
        matches!(error, EditError::IdInUse(SceneEntityId(2))),
        "{error}"
    );

    let error = document
        .apply(EditCommand::Spawn {
            entity: SceneEntityId(9),
            rows: vec![SystemRow {
                system: "bricks".to_owned(),
                row,
            }],
            name: None,
        })
        .expect_err("the greybox scene has no bricks");
    assert!(
        matches!(&error, EditError::NoSystem(system) if system == "bricks"),
        "{error}"
    );

    let error = document
        .apply(EditCommand::Spawn {
            entity: SceneEntityId(9),
            rows: vec![SystemRow {
                system: crate::scene::BLOCKS.to_owned(),
                row: "(position:(0.0,0.0,0.0))".to_owned(),
            }],
            name: None,
        })
        .expect_err("a block has half extents");
    assert!(matches!(error, EditError::Scene(_)), "{error}");

    let error = document
        .delete(&[SceneEntityId(9_999)])
        .expect_err("no such entity");
    assert!(matches!(error, EditError::NoEntity(_)), "{error}");

    assert_eq!(document.log().len(), 0, "a refused edit was recorded");
    assert_eq!(document.entity_count(), 4);
    assert_eq!(document.files().expect("ids"), before);
}

/// One clipping holding every entity in `ids`: a copy of them all.
fn clipping_of(document: &mut Document, ids: &[SceneEntityId]) -> String {
    document.copy(ids).expect("held entities")
}

/// **A paste spawns every entity the clipping names under fresh ids, and one
/// undo takes all of them back.**
#[test]
fn a_paste_spawns_every_entity_and_one_undo_takes_them_back() {
    let mut document = document();
    let before = document.files().expect("ids");
    let text = clipping_of(&mut document, &[SceneEntityId(1), SceneEntityId(3)]);

    let pasted = document
        .paste(&text)
        .expect("a clipping of this scene's blocks");
    assert_eq!(pasted, [SceneEntityId(4), SceneEntityId(5)]);
    assert_eq!(document.entity_count(), 6);
    for (copy, original) in [(4, 1), (5, 3)] {
        assert_eq!(
            document
                .read(SceneEntityId(copy), crate::scene::BLOCKS, "position.0")
                .expect("pasted"),
            document
                .read(SceneEntityId(original), crate::scene::BLOCKS, "position.0")
                .expect("held"),
        );
    }
    assert_eq!(document.log().len(), 1, "a paste is one entry");

    assert!(document.undo().expect("the batch's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
}

/// **A paste with one entity the scene cannot hold spawns none of them** —
/// including the ones before it, which the batch puts back — and records
/// nothing.
#[test]
fn a_paste_with_one_bad_entity_spawns_none() {
    let mut document = document();
    let before = document.files().expect("ids");
    let good = document.copy(&[SceneEntityId(2)]).expect("held");
    let mut entities = crate::clipboard::decode(&good).expect("its own copy");
    let clipped = |system: &str, row: &str| crate::clipboard::Clipped {
        system: system.to_owned(),
        row: row.to_owned(),
        others: Vec::new(),
        name: None,
    };
    for bad in [
        clipped("bricks", &entities[0].row),
        clipped("blocks", "(position:(0.0,0.0,0.0))"),
    ] {
        entities.truncate(1);
        entities.push(bad);
        let text = crate::clipboard::encode(entities.clone());
        document
            .paste(&text)
            .expect_err("the second entity cannot be spawned");
        assert_eq!(
            document.entity_count(),
            4,
            "the first entity was left behind"
        );
        assert_eq!(document.files().expect("ids"), before);
        assert!(document.log().is_empty());
    }

    let error = document.paste("hello").expect_err("not a clipping");
    assert!(matches!(error, EditError::Paste(_)), "{error}");
}
