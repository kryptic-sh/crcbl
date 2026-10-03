//! The history beside a scene on disk: read back across documents, bound to
//! the scene's bytes, bounded, and refused whole when anything in it is off.

use super::tests::{BLOCKS, one_block, vocabulary};
use super::*;
use crate::shaders::sha256::sha256;

/// Where the layout's version, position and count start.
const VERSION_AT: usize = 8;
const POSITION_AT: usize = VERSION_AT + 2 + 32;
const COUNT_AT: usize = POSITION_AT + 4;

/// The one block's scene saved into a fresh directory under `base`, as the
/// document whose origin it is.
fn saved(base: &tempfile::TempDir) -> (PathBuf, Document) {
    let dir = base.path().join("one.scn");
    let mut document = one_block();
    document.save_as(&dir).expect("a fresh directory");
    (dir, document)
}

/// The block moved to `x`, as one entry.
fn moved(x: f64) -> EditCommand {
    EditCommand::SetProperty {
        entity: SceneEntityId(0),
        system: BLOCKS.to_owned(),
        path: "position.0".to_owned(),
        value: Value::Float(x),
    }
}

/// The same scene opened again with its history.
fn reopened(dir: &Path) -> Result<Document, EditError> {
    Document::open_with_history(dir, vocabulary())
}

/// `bytes` with their checksum made right again, so a check after it is the
/// one that has to refuse.
fn resealed(mut bytes: Vec<u8>) -> Vec<u8> {
    let body = bytes.len() - 32;
    let checksum = sha256(&bytes[..body]);
    bytes[body..].copy_from_slice(&checksum);
    bytes
}

/// The history error `result` refused with.
fn refusal(result: Result<Document, EditError>) -> HistoryError {
    match result {
        Err(EditError::History(error)) => error,
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(_) => panic!("the history was taken"),
    }
}

/// **An edit saved with its history undoes in another document**, the files
/// back byte for byte, and the undo's own save redoes in a third.
#[test]
fn an_edit_saved_with_its_history_undoes_and_redoes_in_other_documents() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = saved(&base);
    let before = document.files().expect("ids");

    let mut first = reopened(&dir).expect("no history is a fresh one");
    assert!(first.log().is_empty());
    first.apply(moved(4.0)).expect("a block moves");
    first.save_with_history().expect("saved");
    let after = first.files().expect("ids");

    let mut second = reopened(&dir).expect("its own history");
    assert_eq!((second.log().position(), second.log().len()), (1, 1));
    assert!(!second.is_dirty(), "a history read back opens clean");
    assert!(second.undo().expect("the inverse applies"));
    assert_eq!(second.files().expect("ids"), before);
    second.save_with_history().expect("saved");

    let mut third = reopened(&dir).expect("its own history");
    assert_eq!((third.log().position(), third.log().len()), (0, 1));
    assert_eq!(third.files().expect("ids"), before);
    assert!(third.redo().expect("the command applies"));
    assert_eq!(third.files().expect("ids"), after);
}

/// **A history beside a scene changed since is refused**, and the scene is
/// not touched.
#[test]
fn a_history_beside_a_changed_scene_is_refused() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = saved(&base);
    document.apply(moved(4.0)).expect("a block moves");
    document.save_with_history().expect("saved");

    let mut elsewhere = Document::open_dir(&dir, vocabulary()).expect("opens");
    elsewhere.apply(moved(9.0)).expect("a block moves");
    elsewhere.save().expect("saved without a history");
    let changed = elsewhere.files().expect("ids");

    assert!(matches!(
        refusal(reopened(&dir)),
        HistoryError::SceneChanged
    ));
    let mut plain = Document::open_dir(&dir, vocabulary()).expect("opens");
    assert_eq!(plain.files().expect("ids"), changed);
}

/// **A history is refused for any byte out of place**: a changed byte by
/// its checksum, and — sealed again so the checksum holds — a version this
/// build does not read, a position past its entries, and more entries than
/// a history keeps.
#[test]
fn a_history_is_refused_for_any_byte_out_of_place() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = saved(&base);
    document.apply(moved(4.0)).expect("a block moves");
    document.save_with_history().expect("saved");
    let path = dir.join(HISTORY);
    let good = std::fs::read(&path).expect("a history");
    assert_eq!(good[POSITION_AT], 1, "the layout moved");

    let refused = |bytes: Vec<u8>| {
        std::fs::write(&path, bytes).expect("written");
        refusal(reopened(&dir))
    };
    let mut flipped = good.clone();
    flipped[POSITION_AT] = 0;
    assert!(matches!(refused(flipped), HistoryError::Checksum));

    let mut cut = good.clone();
    cut.truncate(VERSION_AT);
    assert!(matches!(refused(cut), HistoryError::Malformed(_)));

    let mut version = good.clone();
    version[VERSION_AT] = 1;
    assert!(matches!(
        refused(resealed(version)),
        HistoryError::Version(1)
    ));

    let mut position = good.clone();
    position[POSITION_AT] = 2;
    assert!(matches!(
        refused(resealed(position)),
        HistoryError::Position {
            position: 2,
            count: 1
        }
    ));

    let mut count = good.clone();
    count[COUNT_AT] = u8::try_from(MAX_HISTORY_ENTRIES + 1).expect("a small bound");
    assert!(matches!(
        refused(resealed(count)),
        HistoryError::TooMany(too_many) if too_many == MAX_HISTORY_ENTRIES + 1
    ));

    std::fs::write(&path, &good).expect("written");
    assert!(reopened(&dir).is_ok(), "the untouched history is refused");
}

/// **A history past its size is refused before it is decoded.**
#[test]
fn a_history_past_its_size_is_refused_unread() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, _document) = saved(&base);
    let mut bytes = b"CRCBLHIS".to_vec();
    bytes.resize(MAX_HISTORY_BYTES + 1, 0);
    std::fs::write(dir.join(HISTORY), bytes).expect("written");
    assert!(matches!(refusal(reopened(&dir)), HistoryError::TooLarge));
}

/// **A history keeps its newest entries**: written past the bound, the
/// oldest applied ones go, and undoing every entry kept reaches the scene as
/// it stood after the ones dropped.
#[test]
fn a_history_keeps_its_newest_entries() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = saved(&base);
    let extra = 5;
    let mut states = Vec::new();
    for step in 0..MAX_HISTORY_ENTRIES + extra {
        states.push(document.files().expect("ids"));
        let x = f64::from(u32::try_from(step).expect("a small count")) + 1.0;
        document.apply(moved(x)).expect("a block moves");
    }
    document.save_with_history().expect("saved");

    let mut kept = reopened(&dir).expect("its own history");
    assert_eq!(kept.log().len(), MAX_HISTORY_ENTRIES);
    assert_eq!(kept.log().position(), MAX_HISTORY_ENTRIES);
    while kept.undo().expect("every inverse applies") {}
    assert_eq!(kept.files().expect("ids"), states[extra]);
}
