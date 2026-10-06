//! `crcbl edit --serve` following its scene's chunk files on disk, through
//! the rig of the serve tests (`tests.rs`): a chunk another program changes
//! is reloaded, saved and followed by every client with no fetch, and one
//! changed while a client's drag is open waits for the drag to end and is
//! then taken over its unsaved edits. The module docs hold the decisions.

use std::collections::BTreeMap;

use crcbl::net::{EditGesture, EditOutcome};
use crcbl::reflect::Value;
use crcbl::scene::edit::{EditCommand, EditOp};
use crcbl::scene::scn::SceneEntityId;

use super::tests::{FRAME, Rig, TempDir, field_copy, move_entry, scene_files};
use super::*;

/// Plot 6 in towers' field: what another program relabels here.
const FAR_PLOT: SceneEntityId = SceneEntityId(6);

/// Another program's edit, made without the lock — a text editor, an older
/// build: the scene at `dir` opened, plot 6 relabelled `far`, saved. Hands
/// back the files it left.
fn relabelled_behind(dir: &Path) -> BTreeMap<String, String> {
    let mut other = Document::open_dir(dir, crcbl_editor::scene::vocabulary()).expect("the field");
    other
        .apply(EditCommand::SetProperty {
            entity: FAR_PLOT,
            system: "plots".to_owned(),
            path: "label".to_owned(),
            value: Value::Text("far".to_owned()),
        })
        .expect("a label");
    other.save().expect("a save behind the lock");
    scene_files(dir)
}

/// Steps the rig for as long as the watch takes to offer a change written
/// before the first step — a look, [`crcbl::assets::watch::SETTLE`] and the
/// look that offers it — and twice that again for margin.
fn settle(rig: &mut Rig) {
    let latest = crcbl::assets::watch::POLL_INTERVAL * 2 + crcbl::assets::watch::SETTLE;
    for _ in 0..2 * latest.as_nanos() / FRAME.as_nanos() {
        rig.step();
    }
}

/// **A chunk another program changed is reloaded, saved and followed with
/// no fetch**: the served scene and every copy become the other program's,
/// the server's own save is no second change, and an undo over the protocol
/// walks the reload back on disk and in the copy.
#[test]
fn a_chunk_changed_on_disk_is_reloaded_saved_and_followed_without_a_fetch() {
    let temp = TempDir::new("watch");
    let dir = field_copy(&temp, "field.scn");
    let before = scene_files(&dir);
    let mut rig = Rig::serving(&dir);
    let reader = rig.join();
    rig.caught_up();

    let theirs = relabelled_behind(&dir);
    assert_ne!(theirs, before);
    rig.until("the server reloads the chunk", |rig| {
        rig.server.edit.revision() == 1
    });
    rig.caught_up();
    for copy in rig.copies() {
        assert_eq!(copy, theirs, "a copy did not follow the reload");
    }
    assert_eq!(
        rig.clients[reader].follower.fetch_count(),
        1,
        "the copy fetched rather than followed"
    );
    assert_eq!(scene_files(&dir), theirs);
    let status = rig.server.status();
    assert!(status.ends_with("saved"), "{status}");
    settle(&mut rig);
    assert_eq!(
        rig.server.edit.revision(),
        1,
        "the server's own save was taken for a change"
    );

    assert_eq!(
        rig.send(reader, &EditOp::Undo),
        EditOutcome::Applied { revision: 2 }
    );
    assert_eq!(scene_files(&dir), before, "the undo was not saved");
    rig.caught_up();
    for copy in rig.copies() {
        assert_eq!(copy, before, "a copy did not follow the undo");
    }
}

/// **A chunk changed while a client's drag is open waits for the drag to
/// end**, and is then taken over the drag's unsaved edits, before the save:
/// nothing is reloaded or written mid-drag, the update that ends the drag
/// reloads and saves the other program's chunk, and the drag is still one
/// entry beneath the reload, which an undo walks back.
#[test]
fn a_chunk_changed_mid_drag_is_reloaded_over_it_once_the_drag_ends() {
    let temp = TempDir::new("watch-drag");
    let dir = field_copy(&temp, "field.scn");
    let expected_dir = field_copy(&temp, "expected.scn");
    let mut rig = Rig::serving(&dir);
    let author = rig.join();
    rig.caught_up();

    assert_eq!(
        rig.send_in(
            author,
            &EditOp::Apply(move_entry(1.5)),
            EditGesture { id: 1, last: false }
        ),
        EditOutcome::Applied { revision: 1 }
    );
    let theirs = relabelled_behind(&dir);
    settle(&mut rig);
    assert_eq!(rig.server.edit.revision(), 1, "reloaded under the drag");
    assert_eq!(scene_files(&dir), theirs, "the server wrote mid-drag");

    assert_eq!(
        rig.send_in(
            author,
            &EditOp::Apply(move_entry(2.0)),
            EditGesture { id: 1, last: true }
        ),
        EditOutcome::Applied { revision: 2 }
    );
    assert_eq!(
        rig.server.edit.revision(),
        3,
        "the drag's end reloaded nothing"
    );
    assert_eq!(
        scene_files(&dir),
        theirs,
        "the served scene is not the disk's"
    );
    let status = rig.server.status();
    assert!(status.ends_with("saved"), "{status}");
    let log = rig.server.edit.document().log();
    assert_eq!(
        (log.position(), log.len()),
        (2, 2),
        "the drag and the reload"
    );

    assert_eq!(
        rig.send(author, &EditOp::Undo),
        EditOutcome::Applied { revision: 4 }
    );
    let mut expected =
        Document::open_dir(&expected_dir, crcbl_editor::scene::vocabulary()).expect("the field");
    expected.apply(move_entry(2.0)).expect("a plot moves");
    expected.save().expect("the copy saves");
    assert_eq!(
        scene_files(&dir),
        scene_files(&expected_dir),
        "the undo did not bring the drag back"
    );
}
