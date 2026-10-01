//! Spawn, delete and duplicate: the edits that change which entities a
//! document holds, and the property test that walks random histories of them
//! back.

use super::*;

use crcbl::core::rand::{hash_u64, hash_unit};

fn document() -> Document {
    Document::built_in().expect("the compiled-in scene is a scene")
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
        .delete(first)
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
        .duplicate(SceneEntityId(3))
        .expect("the third step is in the scene");
    assert_eq!(copy, SceneEntityId(4), "one past the file's highest id");
    assert_eq!(document.entity_count(), 5);
    for field in ["position", "half_extents"] {
        for axis in 0..3 {
            let path = format!("{field}.{axis}");
            assert_eq!(
                document.read(copy, &path).expect("the copy is a block"),
                document.read(SceneEntityId(3), &path).expect("a block"),
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
            path: "position.0".to_owned(),
            value: Value::Float(-4.0),
        })
        .expect("a block has an x");
    document
        .delete(first)
        .expect("the first step is in the scene");
    document.undo().expect("restore it");
    document.undo().expect("and walk the move back");
    assert_eq!(
        document.read(first, "position.0").expect("restored"),
        Value::Float(-3.0),
    );
}

/// Deleting the selected entity clears the selection, so nothing points at a
/// hole.
#[test]
fn deleting_the_selection_clears_it() {
    let mut document = document();
    document.select(Some(SceneEntityId(2)));
    document.delete(SceneEntityId(2)).expect("in the scene");
    assert_eq!(document.selected(), None);
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
            system: crate::scene::BLOCKS.to_owned(),
            row: row.clone(),
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
            system: "bricks".to_owned(),
            row,
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
            system: crate::scene::BLOCKS.to_owned(),
            row: "(position:(0.0,0.0,0.0))".to_owned(),
            name: None,
        })
        .expect_err("a block has half extents");
    assert!(matches!(error, EditError::Scene(_)), "{error}");

    let error = document
        .delete(SceneEntityId(9_999))
        .expect_err("no such entity");
    assert!(matches!(error, EditError::NoEntity(_)), "{error}");

    assert_eq!(document.log().len(), 0, "a refused edit was recorded");
    assert_eq!(document.entity_count(), 4);
    assert_eq!(document.files().expect("ids"), before);
}

/// One clipping holding every entity in `ids`, as a copy of several would.
fn clipping_of(document: &mut Document, ids: &[SceneEntityId]) -> String {
    let entities = ids
        .iter()
        .flat_map(|id| {
            let text = document.copy(*id).expect("a held entity");
            crate::clipboard::decode(&text).expect("its own copy")
        })
        .collect();
    crate::clipboard::encode(entities)
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
                .read(SceneEntityId(copy), "position.0")
                .expect("pasted"),
            document
                .read(SceneEntityId(original), "position.0")
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
    let good = document.copy(SceneEntityId(2)).expect("held");
    let mut entities = crate::clipboard::decode(&good).expect("its own copy");
    let clipped = |system: &str, row: &str| crate::clipboard::Clipped {
        system: system.to_owned(),
        row: row.to_owned(),
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

/// How many random histories [`random_histories_walk_back_through_every_state`]
/// plays, and how long each one is.
const HISTORIES: u64 = 48;
const HISTORY_LEN: u64 = 24;

/// What a rename in [`random_histories_walk_back_through_every_state`] names
/// an entity — one of these, or nothing.
const NAMES: [Option<&str>; 4] = [Some("Gate"), Some("Spawner"), Some("Tower"), None];

/// **`docs/plan/08-editor.md`'s undo property test: a random sequence of
/// commands, undone in full, lands on the state it started from** — and on every
/// state in between on the way down, and back up again on redo.
///
/// The state compared is the scene's saved text, not
/// [`crcbl::ecs::World::hash_state`]: the world hash folds in each
/// [`Entity`]'s bits, and a restored entity is a new `Entity` filed under its
/// old [`SceneEntityId`] — the same scene with a different handle. The text is
/// keyed by the id and prints every float through its shortest round trip, so
/// two states that differ in any field or any row differ here.
///
/// Each step is one of a nudge by an arbitrary float, a duplicate, a delete, a
/// paste of two entities as one batch, or a rename — to one of a few names, or
/// to none — of entities chosen from those the document holds at that moment.
/// The text compared includes `names.ron` and the header's `names` entry, so a
/// name an undo failed to put back, or a delete failed to take away, differs
/// here. The
/// choices come from [`hash_u64`] over `(seed, index)`, so a failure names the
/// history that produced it and replays exactly.
#[test]
fn random_histories_walk_back_through_every_state() {
    // How many states held a name, so a run whose renames never landed in a
    // file cannot pass as one that walked them back.
    let mut named = 0;
    for seed in 0..HISTORIES {
        let mut document = document();
        let mut states = vec![document.files().expect("ids")];
        for step in 0..HISTORY_LEN {
            let draw = |k: u64| hash_u64(seed, step * 4 + k);
            let held = ids(&mut document);
            let len = u64::try_from(held.len()).expect("a handful of entities");
            let target = held[usize::try_from(draw(0) % len).expect("an index into held")];
            match draw(1) % 6 {
                0 if held.len() > 1 => document.delete(target).expect("a held entity"),
                1 => {
                    document.duplicate(target).expect("a held entity");
                }
                3 => {
                    let name = NAMES[usize::try_from(draw(2) % 4).expect("an index into NAMES")]
                        .map(|name| EntityName::new(name).expect("a name"));
                    document
                        .apply(EditCommand::Rename {
                            entity: target,
                            name,
                        })
                        .expect("a held entity");
                }
                2 => {
                    let other = held[usize::try_from(draw(2) % len).expect("an index into held")];
                    let text = clipping_of(&mut document, &[target, other]);
                    assert_eq!(document.paste(&text).expect("a clipping").len(), 2);
                }
                _ => {
                    let axis = draw(2) % 3;
                    let path = format!("position.{axis}");
                    let Value::Float(was) = document.read(target, &path).expect("a block") else {
                        panic!("a position is a float");
                    };
                    let delta = (hash_unit(seed, step * 4 + 3) - 0.5) * 4.0;
                    document
                        .apply(EditCommand::SetProperty {
                            entity: target,
                            path,
                            value: Value::Float(was + delta),
                        })
                        .expect("a block has that axis");
                }
            }
            states.push(document.files().expect("ids"));
        }

        for (position, state) in states.iter().enumerate().rev().skip(1) {
            assert!(document.undo().expect("each entry's inverse applies"));
            assert_eq!(
                &document.files().expect("ids"),
                state,
                "seed {seed}: undoing to position {position} did not restore that state",
            );
        }
        assert!(!document.undo().expect("nothing left"), "seed {seed}");
        assert_eq!(document.entity_count(), 4, "seed {seed}");
        assert!(!document.is_dirty(), "seed {seed}");

        for (position, state) in states.iter().enumerate().skip(1) {
            assert!(document.redo().expect("each entry applies again"));
            assert_eq!(
                &document.files().expect("ids"),
                state,
                "seed {seed}: redoing to position {position} did not restore that state",
            );
        }
        named += states
            .iter()
            .filter(|state| state.contains_key("names.ron"))
            .count();
    }
    assert!(named > 0, "no history named an entity");
}
