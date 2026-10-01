//! Spawn, delete and duplicate: the edits that change which entities a
//! document holds, and the property test that walks random histories of them
//! back.

use super::*;

use super::systems_tests::{SUN, two_systems};
use crate::command::SystemRow;

use crcbl::scene_mesh::MESHES;
use crcbl::scene_physics::BODIES;

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
        .delete(first)
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
    let good = document.copy(SceneEntityId(2)).expect("held");
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
/// paste of two entities as one batch, a rename — to one of a few names, or
/// to none — an attach of a system the entity is not in, a detach of one of
/// several it is in, a drop of a mesh asset (listing `meshes` in the same
/// entry when the manifest lacks it), a listing of a registered system at a
/// random place in the manifest, an unlisting of a listed system holding no
/// entity, and an attach to a system the manifest does not list — of
/// entities chosen from those the document holds at that moment. The
/// document is [`two_systems`]', so an entity starts in two systems and a
/// delete, duplicate or paste of it carries both rows. The text compared is
/// every file: `scene.ron`, so a manifest entry an undo failed to take out or
/// put back in its place differs here; `names.ron` and the header's `names`
/// entry, so a name an undo failed to put back, or a delete failed to take
/// away, differs; and every chunk, so a row an undone detach failed to put
/// back differs too. The choices come from [`hash_u64`] over `(seed, index)`,
/// so a failure names the history that produced it and replays exactly.
#[test]
fn random_histories_walk_back_through_every_state() {
    // How many times each kind of step that can be skipped for want of a
    // target ran, so a run that never reached one cannot pass as one that
    // walked it back.
    let mut ran = Ran::default();
    for seed in 0..HISTORIES {
        let mut document = two_systems();
        document.set_assets(Box::new(super::mesh_tests::assets()));
        let start = document.entity_count();
        let mut states = vec![document.files().expect("ids")];
        for step in 0..HISTORY_LEN {
            let draw = |k: u64| hash_u64(seed, step * DRAWS + k);
            let held = ids(&mut document);
            let len = u64::try_from(held.len()).expect("a handful of entities");
            let target = held[usize::try_from(draw(0) % len).expect("an index into held")];
            let joinable = document.attachable(target);
            let systems = document.systems_of(target);
            let listed = document.scene.systems().to_vec();
            let unlisted: Vec<String> = document
                .registry
                .systems()
                .filter(|system| !listed.iter().any(|each| each == system))
                .map(str::to_owned)
                .collect();
            let empty: Vec<String> = listed
                .iter()
                .filter(|system| {
                    document
                        .registry
                        .entities(&mut document.world, &document.ids, system)
                        .is_empty()
                })
                .cloned()
                .collect();
            let joinable_unlisted: Vec<&String> = joinable
                .iter()
                .filter(|system| unlisted.contains(system))
                .collect();
            match draw(1) % 12 {
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
                4 if !joinable.is_empty() => {
                    let system = &joinable[pick(draw(2), joinable.len())];
                    document
                        .attach(target, system)
                        .expect("a system it is not in");
                    ran.attached += 1;
                }
                5 if systems.len() > 1 => {
                    let system = &systems[pick(draw(2), systems.len())];
                    document
                        .detach(target, system)
                        .expect("one of several systems");
                    ran.detached += 1;
                }
                6 => {
                    let asset = DROPPED[pick(draw(2), DROPPED.len())];
                    let point = DVec3::new(
                        (hash_unit(seed, step * DRAWS + 3) - 0.5) * DROP_SPREAD,
                        0.0,
                        (hash_unit(seed, step * DRAWS + 4) - 0.5) * DROP_SPREAD,
                    );
                    if !listed.iter().any(|system| system == MESHES) {
                        ran.dropped_listing += 1;
                    }
                    document.spawn_mesh(asset, point).expect("a mesh asset key");
                    ran.dropped += 1;
                }
                7 if !unlisted.is_empty() => {
                    let system = unlisted[pick(draw(2), unlisted.len())].clone();
                    let at = pick(draw(3), listed.len() + 1);
                    document
                        .apply(EditCommand::ListSystem { system, at })
                        .expect("a registered system the manifest lacks");
                    ran.listed += 1;
                }
                8 if !empty.is_empty() => {
                    let system = empty[pick(draw(2), empty.len())].clone();
                    if listed.last() != Some(&system) {
                        ran.unlisted_inside += 1;
                    }
                    document
                        .apply(EditCommand::UnlistSystem { system })
                        .expect("a listed system holding no entity");
                    ran.unlisted += 1;
                }
                9 if !joinable_unlisted.is_empty() => {
                    let system = joinable_unlisted[pick(draw(2), joinable_unlisted.len())];
                    document
                        .attach(target, system)
                        .expect("a registered system it is not in");
                    ran.attached_unlisted += 1;
                }
                _ => {
                    let (system, path) = nudged_leaf(&mut document, target, draw(2));
                    let Value::Float(was) = document.read(target, &system, &path).expect("a leaf")
                    else {
                        panic!("{path} is a float");
                    };
                    let delta = (hash_unit(seed, step * DRAWS + 5) - 0.5) * 4.0;
                    document
                        .apply(EditCommand::SetProperty {
                            entity: target,
                            system,
                            path,
                            value: Value::Float(was + delta),
                        })
                        .expect("the component has that leaf");
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
        assert_eq!(document.entity_count(), start, "seed {seed}");
        assert!(!document.is_dirty(), "seed {seed}");

        for (position, state) in states.iter().enumerate().skip(1) {
            assert!(document.redo().expect("each entry applies again"));
            assert_eq!(
                &document.files().expect("ids"),
                state,
                "seed {seed}: redoing to position {position} did not restore that state",
            );
        }
        ran.named += states
            .iter()
            .filter(|state| state.contains_key("names.ron"))
            .count();
    }
    ran.assert_every_kind_ran();
}

/// How many draws one step of [`random_histories_walk_back_through_every_state`]
/// takes from [`hash_u64`] and [`hash_unit`]: the indices `step * DRAWS + k`
/// never overlap between steps.
const DRAWS: u64 = 8;

/// The width of the square about the origin a drop in
/// [`random_histories_walk_back_through_every_state`] lands in.
const DROP_SPREAD: f64 = 16.0;

/// The assets a drop in [`random_histories_walk_back_through_every_state`]
/// places: one the asset root holds, and one it does not (a placeholder).
const DROPPED: [&str; 2] = [super::mesh_tests::TRIANGLE, super::mesh_tests::GONE];

/// What [`random_histories_walk_back_through_every_state`] counts, each kind
/// of step that may be skipped for want of a target.
#[derive(Default)]
struct Ran {
    /// States that held a name.
    named: usize,
    attached: usize,
    detached: usize,
    dropped: usize,
    /// Drops into a manifest without `meshes`, which list it too.
    dropped_listing: usize,
    listed: usize,
    unlisted: usize,
    /// Unlistings of a system that was not the manifest's last, whose undo
    /// has to put it back where it was rather than at the end.
    unlisted_inside: usize,
    attached_unlisted: usize,
}

impl Ran {
    /// Panics naming the first kind of step no history ran.
    fn assert_every_kind_ran(&self) {
        for (count, what) in [
            (self.named, "no history named an entity"),
            (self.attached, "no history attached a system"),
            (self.detached, "no history detached a system"),
            (self.dropped, "no history dropped a mesh"),
            (self.dropped_listing, "no drop listed meshes"),
            (self.listed, "no history listed a system"),
            (self.unlisted, "no history unlisted a system"),
            (self.unlisted_inside, "no unlisting was of a middle system"),
            (
                self.attached_unlisted,
                "no history attached to an unlisted system",
            ),
        ] {
            assert!(count > 0, "{what}");
        }
    }
}

/// The leaf a nudge of `target` moves, from the draw `value`: its placing
/// component's position across the ground — `X` or `Z`, never the height,
/// which towers' row rules hold its plots and corners to — or for an entity
/// nothing places a float of a component it holds: a sun's period, else a
/// body's friction.
fn nudged_leaf(document: &mut Document, target: SceneEntityId, value: u64) -> (String, String) {
    if let Some(system) = document.placing_system(target) {
        let axis = if value.is_multiple_of(2) { 0 } else { 2 };
        return (system, format!("position.{axis}"));
    }
    if document
        .systems_of(target)
        .iter()
        .any(|system| system == SUN)
    {
        (SUN.to_owned(), "period".to_owned())
    } else {
        (BODIES.to_owned(), "friction".to_owned())
    }
}

/// An index below `len` from the draw `value`.
fn pick(value: u64, len: usize) -> usize {
    usize::try_from(value % u64::try_from(len).expect("a handful of systems"))
        .expect("an index below len")
}
