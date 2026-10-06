//! A chunk file reloaded from the document's own directory: what it changes
//! and what it leaves, and the rule against unsaved edits —
//! `scene_edit::reload`'s module docs hold the decisions.

use super::tests::{BLOCKS, Block, vocabulary};
use super::*;
use crate::scene::scn::RowChange;

/// The second system, holding the same component as [`BLOCKS`], so one
/// entity can be in both.
const PADS: &str = "pads";

const HEADER: &str = "Scene(format: 0, name: \"two\", systems: [\"blocks\", \"pads\"])";

/// Blocks 0 and 1.
const BLOCKS_RON: &str = "Chunk(system: \"blocks\", entities: [\
    (0, Block(position: (0.0, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0))),\
    (1, Block(position: (1.0, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0))),\
])";

/// Pads 1 and 2: entity 1 is in both systems.
const PADS_RON: &str = "Chunk(system: \"pads\", entities: [\
    (1, Block(position: (10.0, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0))),\
    (2, Block(position: (20.0, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0))),\
])";

/// [`vocabulary`] and [`PADS`].
fn two_systems() -> Registry {
    let mut registry = vocabulary();
    registry.register::<Block>(PADS);
    registry
}

/// The two-system scene with entity 0 named `Gate`, saved into a fresh
/// directory under `base` and opened from there: a clean document whose
/// directory it is.
fn opened(base: &tempfile::TempDir) -> (PathBuf, Document) {
    let mut source = empty_source();
    for (key, text) in [
        ("scene.ron", HEADER),
        ("sys/blocks.ron", BLOCKS_RON),
        ("sys/pads.ron", PADS_RON),
    ] {
        source
            .insert(Path::new(key), text.as_bytes().to_vec())
            .expect("a scene key is a legal asset key");
    }
    let mut document =
        Document::open(&source, Path::new(""), two_systems()).expect("two systems are a scene");
    assert!(document.rename(SceneEntityId(0), "Gate").expect("a name"));
    let dir = base.path().join("two.scn");
    document.save_as(&dir).expect("a fresh directory");
    let document = Document::open_dir(&dir, two_systems()).expect("the saved scene opens");
    assert!(!document.is_dirty());
    (dir, document)
}

/// Writes `text` as `dir`'s blocks chunk, as a text editor would.
fn write_blocks(dir: &Path, text: &str) {
    std::fs::write(dir.join("sys/blocks.ron"), text).expect("the chunk written");
}

/// Block `id`'s x in `system`.
fn x_of(document: &mut Document, id: u32, system: &str) -> Option<Value> {
    document.read(SceneEntityId(id), system, "position.0").ok()
}

/// **A clean document reloads one chunk as one entry and stays clean**: the
/// other system is untouched to the entity, the save that follows is not
/// refused over the change it took in, and an undo walks the reload back.
#[test]
fn a_clean_document_reloads_one_chunk_as_one_entry_and_stays_clean() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    let pads_before = document.files().expect("ids")["sys/pads.ron"].clone();
    let pad = document.ids().entity(SceneEntityId(2));

    write_blocks(
        &dir,
        &BLOCKS_RON.replace("(1.0, 0.0, 0.0)", "(4.0, 0.0, 0.0)"),
    );
    let reloaded = document
        .reload_chunk(BLOCKS, OverEdits::Refuse)
        .expect("a clean document reloads");

    assert_eq!(
        reloaded
            .diff
            .changes()
            .iter()
            .map(RowChange::id)
            .collect::<Vec<_>>(),
        [SceneEntityId(1)],
        "only the row that changed"
    );
    assert!(reloaded.command.is_some());
    assert_eq!(x_of(&mut document, 1, BLOCKS), Some(Value::Float(4.0)));
    assert_eq!(x_of(&mut document, 1, PADS), Some(Value::Float(10.0)));
    assert_eq!(document.files().expect("ids")["sys/pads.ron"], pads_before);
    assert_eq!(
        document.ids().entity(SceneEntityId(2)),
        pad,
        "a pad was re-made"
    );
    assert_eq!(document.log().len(), 1, "the reload is one entry");
    assert!(
        !document.is_dirty(),
        "a reload of a clean document left it dirty"
    );
    document
        .save()
        .expect("the save does not refuse over the reload");

    assert!(document.undo().expect("the reload walks back"));
    assert_eq!(x_of(&mut document, 1, BLOCKS), Some(Value::Float(1.0)));
    assert!(
        document.is_dirty(),
        "undone, the document differs from the disk"
    );
}

/// **Unsaved edits refuse a reload until the caller says to take the
/// disk's**, and then the reload lands on top of them: the edit is still in
/// the history beneath it, and one undo takes only the reload back.
#[test]
fn unsaved_edits_refuse_a_reload_until_told_to_take_the_disks() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    document
        .apply(EditCommand::SetProperty {
            entity: SceneEntityId(2),
            system: PADS.to_owned(),
            path: "position.0".to_owned(),
            value: Value::Float(25.0),
        })
        .expect("a pad moves");
    let before = document.files().expect("ids");

    write_blocks(
        &dir,
        &BLOCKS_RON.replace("(0.0, 0.0, 0.0)", "(3.0, 0.0, 0.0)"),
    );
    match document.reload_chunk(BLOCKS, OverEdits::Refuse) {
        Err(EditError::Unsaved(system)) => assert_eq!(system, BLOCKS),
        other => panic!("unsaved edits did not refuse the reload: {other:?}"),
    }
    assert_eq!(
        document.files().expect("ids"),
        before,
        "a refusal changed the scene"
    );
    assert_eq!(document.log().len(), 1);

    document
        .reload_chunk(BLOCKS, OverEdits::Reload)
        .expect("told to take the disk's");
    assert_eq!(x_of(&mut document, 0, BLOCKS), Some(Value::Float(3.0)));
    assert_eq!(
        x_of(&mut document, 2, PADS),
        Some(Value::Float(25.0)),
        "the edit went"
    );
    assert_eq!(document.log().len(), 2, "the reload is not an entry on top");
    assert!(document.is_dirty(), "the edit is still unsaved");

    assert!(document.undo().expect("the reload walks back"));
    assert_eq!(x_of(&mut document, 0, BLOCKS), Some(Value::Float(0.0)));
    assert_eq!(x_of(&mut document, 2, PADS), Some(Value::Float(25.0)));
}

/// **Ids survive a reload that keeps them**: a changed row of an entity in
/// both systems keeps its entity, and one this system alone holds keeps its
/// id and its name.
#[test]
fn a_reload_that_keeps_an_id_keeps_the_entity_it_names() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    let shared = document.ids().entity(SceneEntityId(1));

    write_blocks(
        &dir,
        &BLOCKS_RON
            .replace("(0.0, 0.0, 0.0)", "(0.5, 0.0, 0.0)")
            .replace("(1.0, 0.0, 0.0)", "(1.5, 0.0, 0.0)"),
    );
    document
        .reload_chunk(BLOCKS, OverEdits::Refuse)
        .expect("reloads");

    assert_eq!(x_of(&mut document, 0, BLOCKS), Some(Value::Float(0.5)));
    assert_eq!(x_of(&mut document, 1, BLOCKS), Some(Value::Float(1.5)));
    assert_eq!(
        document.ids().entity(SceneEntityId(1)),
        shared,
        "the entity both systems hold was re-made"
    );
    assert_eq!(x_of(&mut document, 1, PADS), Some(Value::Float(10.0)));
    assert_eq!(
        document
            .scene
            .entity_name(SceneEntityId(0))
            .map(EntityName::as_str),
        Some("Gate"),
        "the reloaded entity lost its name"
    );
}

/// **A removed entity goes and an added one appears**, and a row removed
/// from an entity the other system holds takes only that row.
#[test]
fn a_reload_removes_and_adds_entities_by_the_files_ids() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    let text = "Chunk(system: \"blocks\", entities: [\
        (5, Block(position: (5.0, 0.0, 0.0), half_extents: (1.0, 1.0, 1.0))),\
    ])";
    write_blocks(&dir, text);
    document
        .reload_chunk(BLOCKS, OverEdits::Refuse)
        .expect("reloads");

    assert_eq!(
        document.ids().entity(SceneEntityId(0)),
        None,
        "block 0 stayed"
    );
    assert_eq!(document.scene.entity_name(SceneEntityId(0)), None);
    assert_eq!(x_of(&mut document, 5, BLOCKS), Some(Value::Float(5.0)));
    assert!(
        document.ids().entity(SceneEntityId(1)).is_some(),
        "entity 1 went with its block"
    );
    assert_eq!(document.systems_of(SceneEntityId(1)), [PADS]);
    // And the scene saves as the disk now says, under the file's ids.
    document.save().expect("saves");
    let reopened = Document::open_dir(&dir, two_systems()).expect("reopens");
    assert_eq!(reopened.entity_count(), 3);
}

/// **A chunk that will not read keeps the last good state** and says which
/// file, recording nothing.
#[test]
fn a_chunk_that_will_not_read_keeps_the_last_good_state() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    let before = document.files().expect("ids");
    write_blocks(&dir, &BLOCKS_RON[..BLOCKS_RON.len() / 2]);
    match document.reload_chunk(BLOCKS, OverEdits::Refuse) {
        Err(EditError::Scene(ScnError::Parse { key, .. })) => assert_eq!(key, "sys/blocks.ron"),
        other => panic!("half a file was not refused: {other:?}"),
    }
    assert_eq!(document.files().expect("ids"), before);
    assert!(document.log().is_empty());
}

/// **A routed copy does not reload**: its server's document is the one with
/// the directory, and a routed spawn would lose the file's ids.
#[test]
fn a_routed_copy_refuses_a_reload() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (_dir, mut document) = opened(&base);
    document.route_edits();
    assert!(matches!(
        document.reload_chunk(BLOCKS, OverEdits::Reload),
        Err(EditError::Routed)
    ));
    assert!(document.take_routed().is_empty(), "the reload was routed");
}
