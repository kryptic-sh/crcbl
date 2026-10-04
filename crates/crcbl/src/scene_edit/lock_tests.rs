//! The scene lock as a document holds it, and a save refusing to overwrite a
//! scene changed on disk behind the document's back — `scene_edit::lock`'s
//! module docs hold the decisions.

use super::tests::{BLOCKS, one_block, vocabulary};
use super::*;

/// The one block's scene saved into a fresh directory under `base`, as the
/// document whose origin it is.
fn saved(base: &tempfile::TempDir) -> (PathBuf, Document) {
    let dir = base.path().join("one.scn");
    let mut document = one_block();
    document.save_as(&dir).expect("a fresh directory");
    (dir, document)
}

/// The block moved to `x`.
fn moved(x: f64) -> EditCommand {
    EditCommand::SetProperty {
        entity: SceneEntityId(0),
        system: BLOCKS.to_owned(),
        path: "position.0".to_owned(),
        value: Value::Float(x),
    }
}

/// The scene's files in `dir`, as the document's [`Document::files`] keys
/// them.
fn on_disk(dir: &Path, keys: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    keys.keys()
        .map(|key| {
            let text = std::fs::read_to_string(dir.join(key)).expect("a scene file");
            (key.clone(), text)
        })
        .collect()
}

/// Another program's edit, made without the lock: the scene opened again,
/// the block moved to `x`, and saved.
fn edited_behind(dir: &Path, x: f64) -> BTreeMap<String, String> {
    let mut other = Document::open_dir(dir, vocabulary()).expect("the saved scene opens");
    other.apply(moved(x)).expect("a block moves");
    other.save().expect("saved");
    other.files().expect("ids")
}

/// **A scene changed on disk since a save-as is not overwritten**: the save
/// is refused naming the directory and the other program's files stay —
/// and once the change is accepted the save lands, and the next one, with
/// nothing changed since, lands too.
#[test]
fn a_save_over_a_scene_changed_on_disk_is_refused_until_accepted() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = saved(&base);
    let theirs = edited_behind(&dir, 7.0);

    document.apply(moved(2.0)).expect("a block moves");
    match document.save() {
        Err(EditError::ChangedOnDisk(refused)) => assert_eq!(refused, dir),
        other => panic!("a changed scene was not refused: {other:?}"),
    }
    assert_eq!(on_disk(&dir, &theirs), theirs, "the refused save wrote");
    assert!(document.is_dirty(), "a refused save marked it saved");

    document.accept_changes_on_disk();
    document.save().expect("an accepted change is overwritten");
    let ours = document.files().expect("ids");
    assert_eq!(on_disk(&dir, &ours), ours);
    document.apply(moved(3.0)).expect("a block moves");
    document.save().expect("its own save is no outside change");
}

/// **A document opened from a directory remembers what it read**: a change
/// made after the open is refused, and so is a file of its own taken away.
#[test]
fn a_change_after_an_open_and_a_removed_file_are_refused() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, _) = saved(&base);
    let mut document = Document::open_dir(&dir, vocabulary()).expect("opens");
    edited_behind(&dir, 5.0);
    assert!(matches!(document.save(), Err(EditError::ChangedOnDisk(_))));

    let mut document = Document::open_dir(&dir, vocabulary()).expect("opens");
    std::fs::remove_file(dir.join("env.ron")).expect("removed");
    assert!(matches!(document.save(), Err(EditError::ChangedOnDisk(_))));
    assert!(!dir.join("env.ron").exists(), "the refused save wrote");
}

/// **A document holds a lock only on its own directory**: opened locked it
/// refuses every other holder; a save-as moving it lets the old directory go
/// and `lock_origin` takes the new one; a new scene lets that go too.
#[test]
fn the_lock_follows_the_documents_directory() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, _) = saved(&base);
    let mut document =
        Document::open_locked(lock_scene(&dir).expect("free"), vocabulary()).expect("opens");
    assert!(document.holds_lock_on(&dir));
    assert!(matches!(lock_scene(&dir), Err(EditError::Locked { .. })));

    let moved_to = base.path().join("two.scn");
    document.save_as(&moved_to).expect("a fresh directory");
    assert!(!document.holds_lock_on(&dir));
    drop(lock_scene(&dir).expect("the old directory was let go"));
    document.lock_origin().expect("the new directory is free");
    assert!(document.holds_lock_on(&moved_to));
    assert!(matches!(
        lock_scene(&moved_to),
        Err(EditError::Locked { .. })
    ));

    document.new_scene().expect("editing");
    assert!(matches!(document.lock_origin(), Err(EditError::NoOrigin)));
    drop(lock_scene(&moved_to).expect("a new scene let it go"));
}

/// **A scene read again is handed the lock its reader holds**, and nothing
/// else is: a document on another directory keeps none.
#[test]
fn a_scene_read_again_takes_the_lock_over() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, _) = saved(&base);
    let (other_dir, _) = {
        let other = base.path().join("other.scn");
        let mut document = one_block();
        document.save_as(&other).expect("a fresh directory");
        (other, document)
    };
    let mut holder =
        Document::open_locked(lock_scene(&dir).expect("free"), vocabulary()).expect("opens");

    let mut elsewhere = Document::open_dir(&other_dir, vocabulary()).expect("opens");
    holder.hand_lock_to(&mut elsewhere);
    assert!(holder.holds_lock_on(&dir), "another directory took it");

    let mut again = Document::open_with_history_or_fresh(&dir, vocabulary()).expect("opens");
    holder.hand_lock_to(&mut again);
    assert!(
        again.holds_lock_on(&dir),
        "the scene read again has no lock"
    );
    assert!(!holder.holds_lock_on(&dir));
    drop(holder);
    assert!(matches!(lock_scene(&dir), Err(EditError::Locked { .. })));
    drop(again);
    drop(lock_scene(&dir).expect("free once its holder goes"));
}

/// **A recovery copy takes no lock and is stopped by none**: written while
/// the scene is locked, it lands in a directory of its own with no lock file,
/// and the scene stays locked.
#[test]
fn a_recovery_copy_neither_takes_nor_meets_the_lock() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, _) = saved(&base);
    let mut document =
        Document::open_locked(lock_scene(&dir).expect("free"), vocabulary()).expect("opens");
    document.apply(moved(9.0)).expect("a block moves");

    let recovery = base.path().join("recovery");
    let copy = document.write_recovery(&recovery, 1).expect("written");
    assert!(copy.join("scene.ron").is_file());
    assert!(!copy.join(SCENE_LOCK).exists(), "the copy took a lock");
    assert!(matches!(lock_scene(&dir), Err(EditError::Locked { .. })));
    assert!(document.is_dirty(), "the copy marked the scene saved");
}
