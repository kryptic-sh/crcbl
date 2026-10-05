//! Edits a joined editor makes faster than the round trip: nudges that
//! compose, spawns that never ask for one id, and what was spawned selected
//! once it lands — `crcbl::scene_edit`'s `route` module's _Edits that
//! compose_, and the join module's docs.

use super::*;

use crcbl::scene::edit::SystemRow;

/// The blocks the server's document holds that it did not before, in id
/// order — the order the spawns here were given them.
fn new_ids(rig: &mut Rig, before: &[SceneEntityId]) -> Vec<SceneEntityId> {
    let mut ids = rig
        .served
        .edit
        .document_mut()
        .entities_in(crate::scene::BLOCKS);
    ids.retain(|id| !before.contains(id));
    ids.sort_unstable();
    ids
}

/// Every block the server's document holds.
fn held(rig: &mut Rig) -> Vec<SceneEntityId> {
    new_ids(rig, &[])
}

/// **Two nudges before the first lands move by both**: both are sent while
/// the copy still holds the block where it was, and the server's block —
/// and the copy, once it follows — ends exactly where two nudges of an
/// editor of its own scene leave it, each added to what the one before
/// left, in two entries.
#[test]
fn two_nudges_before_the_first_lands_move_by_both() {
    let mut rig = Rig::joined(built_in());
    let x = |document: &mut Document| {
        document
            .read(BLOCK, crate::scene::BLOCKS, "position.0")
            .expect("the block has an x")
    };
    let Value::Float(was) = x(rig.served.edit.document_mut()) else {
        panic!("a block's x is a float");
    };
    rig.editor.document_mut().select(Some(BLOCK));
    for _ in 0..2 {
        rig.editor.act(&Action::Nudge { axis: 0, sign: 1.0 });
        rig.editor.step_join(false);
    }
    assert_eq!(
        rig.served.edit.revision(),
        0,
        "the server has heard nothing"
    );
    rig.until("both nudges apply", |rig| rig.served.edit.revision() == 2);
    rig.until_in_step("the copy follows both");

    let both = Value::Float(was + NUDGE_M + NUDGE_M);
    assert_eq!(x(rig.served.edit.document_mut()), both);
    assert_eq!(x(rig.editor.document_mut()), both);
    assert_eq!(rig.served.edit.document().log().len(), 2);
}

/// **Two quick spawns from one editor both apply**: two entities added
/// before the first's notice comes back — each under the id the copy would
/// hand out next, the same one — become two entities on the server, under
/// ids of their own, and nothing is refused.
#[test]
fn two_quick_spawns_from_one_editor_both_apply() {
    let mut rig = Rig::joined(built_in());
    let before = held(&mut rig);
    for _ in 0..2 {
        rig.editor.add_entity(crate::scene::BLOCKS);
        rig.editor.step_join(false);
    }
    rig.until("both spawns apply", |rig| rig.served.edit.revision() == 2);
    rig.until_in_step("the copy follows both");
    assert_eq!(new_ids(&mut rig, &before).len(), 2);
    assert_ne!(rig.status().1, Tone::Warning, "{}", rig.status().0);
}

/// **Two clients spawning at once both apply, under distinct ids**: the
/// editor and another client each spawn under the same stand-in — the next
/// id both copies would hand out — before the server has stepped, and the
/// server spawns both, each under an id no entity held, which every copy
/// follows from the notices — each naming the ids given — without fetching
/// the scene again. The server deleted its last entity before either fetched, so
/// that stand-in is the deleted entity's id, which the server's history
/// still names: an id a client picked would reuse it.
#[test]
fn two_clients_spawning_at_once_both_apply_with_distinct_ids() {
    let mut served = built_in();
    let fresh = served.ids().next_id();
    let last = SceneEntityId(fresh.0 - 1);
    served.delete(&[last]).expect("the last entity goes");
    let mut rig = Rig::joining(Served::serving(served));
    rig.until_in_step("the copy lands");
    rig.watcher = Some(Watcher::to(rig.served.addr()));
    rig.until_in_step("the watcher follows");
    let before = held(&mut rig);
    let stand_in = rig.editor.document().ids().next_id();
    assert_eq!(stand_in, last, "the copy does not know the deleted id");
    let rows: Vec<SystemRow> = rig
        .served
        .edit
        .document_mut()
        .rows(BLOCK)
        .expect("the block has rows");
    let spawn = EditOp::ApplyFresh(EditCommand::Spawn {
        entity: stand_in,
        rows,
        name: None,
    });

    rig.editor.add_entity(crate::scene::BLOCKS);
    rig.editor.step_join(false);
    rig.watcher
        .as_mut()
        .expect("a watcher")
        .client
        .send_edit(encode_op(&spawn).expect("travels"))
        .expect("in session");
    rig.until("both spawns apply", |rig| rig.served.edit.revision() == 2);
    rig.until_in_step("every copy follows both");

    let spawned = new_ids(&mut rig, &before);
    assert_eq!(spawned.len(), 2, "{spawned:?}");
    assert!(
        spawned.iter().all(|id| id.0 >= fresh.0),
        "{spawned:?} reuses an id below {fresh:?}"
    );
    let joined = rig.editor.joined.as_ref().expect("joined");
    let watcher = rig.watcher.as_ref().expect("a watcher");
    assert_eq!(
        (
            joined.follower().fetch_count(),
            watcher.follower.fetch_count()
        ),
        (1, 1),
        "a copy fetched the scene again"
    );
}

/// **A joined spawn ends selected once it lands**, as a spawn of an editor
/// of its own scene is at once: an added entity, and then the duplicates of
/// two entities, each the selection once its notice is in — the last copy
/// the primary — and nothing new selected before.
#[test]
fn a_joined_spawn_ends_selected_once_it_lands() {
    let mut rig = Rig::joined(built_in());
    let before = held(&mut rig);
    rig.editor.add_entity(crate::scene::BLOCKS);
    assert!(
        rig.editor.document().selection().is_empty(),
        "a stand-in was selected"
    );
    rig.until("the spawn lands selected", |rig| {
        !rig.editor.document().selection().is_empty()
    });
    let added = new_ids(&mut rig, &before);
    assert_eq!(rig.editor.document().selection(), added);

    let pair = [BLOCK, added[0]];
    let before = held(&mut rig);
    rig.editor.document_mut().set_selection(pair);
    rig.editor.act(&Action::Duplicate);
    rig.until("the copies land selected", |rig| {
        rig.editor.document().selection().len() == 2
            && !rig.editor.document().selection().contains(&BLOCK)
    });
    let copies = new_ids(&mut rig, &before);
    assert_eq!(rig.editor.document().selection(), copies);
}
