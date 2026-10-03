//! A body's kind switched as the inspector's variant strip reports it: one
//! entry whose undo puts the kind and every field back bit for bit, saved and
//! read back, played as the new kind — and refused, and put back, where the
//! component's rule refuses the result.

use super::*;

use crcbl::reflect::{Snapshot, set_variant_path, snapshot_path};
use crcbl::scene_physics::{BODIES, Body, BodyKind};

use super::physics_tests::{DROP_Y, FALLING, falling, play_ticks, position, ticks_in};

/// `id`'s body, read out of the world.
fn body(document: &mut Document, id: SceneEntityId) -> Body {
    *document
        .component(id, BODIES)
        .expect("the entity has a body")
        .as_any()
        .downcast_ref::<Body>()
        .expect("the bodies system holds `Body`")
}

/// `body`'s values as bits, so a comparison is exact where `==` is not.
fn bits(body: &Body) -> (BodyKind, [u64; 3]) {
    (
        body.kind,
        [body.mass, body.friction, body.restitution].map(f64::to_bits),
    )
}

/// Switches `id`'s body to `kind` in place and reports it to
/// [`Document::record_edits`], as the inspector's strip does.
fn switch_kind(document: &mut Document, id: SceneEntityId, kind: &str) -> Result<(), EditError> {
    let component = document.component(id, BODIES).expect("a body");
    let before = snapshot_path(component, "kind").expect("a body has a kind");
    set_variant_path(component, "kind", kind).expect("a body kind");
    let after = snapshot_path(component, "kind").expect("a body has a kind");
    let switch = VariantEdit {
        path: "kind".to_owned(),
        before,
        after,
    };
    document.record_edits(id, BODIES, &[], &[switch], None)
}

/// **A switch is one entry, whose undo puts the kind and every field back
/// bit for bit and whose redo switches it again** — and the file says so in
/// each state.
#[test]
fn a_bodys_kind_switch_is_one_entry_undone_and_redone_exactly() {
    let mut document = falling();
    let saved = document.files().expect("the scene saves");
    let was = bits(&body(&mut document, FALLING));
    assert_eq!(was.0, BodyKind::Dynamic);

    switch_kind(&mut document, FALLING, "Static").expect("every kind is a body");
    assert_eq!(document.log().len(), 1, "a switch is not one entry");
    assert!(matches!(
        document.log().applied().last(),
        Some(EditCommand::SetVariant { path, .. }) if path == "kind"
    ));
    assert_eq!(body(&mut document, FALLING).kind, BodyKind::Static);
    assert!(document.is_dirty());

    assert!(document.undo().expect("one entry"));
    assert_eq!(
        bits(&body(&mut document, FALLING)),
        was,
        "the undo is not exact"
    );
    assert_eq!(document.files().expect("the scene saves"), saved);
    assert!(!document.is_dirty());

    assert!(document.redo().expect("one entry above"));
    assert_eq!(body(&mut document, FALLING).kind, BodyKind::Static);
}

/// **A switched kind is saved, read back, and played as the new kind**: the
/// falling block made static stands where it was placed through a second of
/// play, where as a dynamic body it fell.
#[test]
fn a_switched_kind_saves_reads_back_and_plays_as_the_new_kind() {
    let mut document = falling();
    switch_kind(&mut document, FALLING, "Static").expect("every kind is a body");

    let files = document.files().expect("the scene saves");
    let bodies = &files["sys/bodies.ron"];
    let row = format!("({}, Body(\n            kind: Static,", FALLING.0);
    assert!(bodies.contains(&row), "{bodies}");
    let source = memory_source(files).expect("scene keys");
    let mut read = Document::open(&source, Path::new(""), crate::scene::vocabulary())
        .expect("the saved scene opens");
    assert_eq!(body(&mut read, FALLING).kind, BodyKind::Static);

    document.play().expect("every body here is placed");
    let second = ticks_in(&document, 1.0);
    play_ticks(&mut document, second);
    assert_eq!(
        position(&mut document, FALLING)[1].to_bits(),
        DROP_Y.to_bits(),
        "the block made static moved in play",
    );
}

/// **A switch the component's rule refuses is put back and not recorded.** A
/// body's rule holds for every kind, so the refusal here is of a body whose
/// mass was already set past it behind the document's back: the rule is the
/// component's, and a switch leaving it failing is refused as a leaf write
/// would be — through the strip's report and as a command alike.
#[test]
fn a_switch_the_rule_refuses_is_put_back_and_not_recorded() {
    let mut document = falling();
    let component = document.component(FALLING, BODIES).expect("a body");
    set_path(component, "mass", &Value::Float(0.0)).expect("a body's mass");

    let error = switch_kind(&mut document, FALLING, "Kinematic").expect_err("no mass");
    assert!(
        matches!(&error, EditError::Invalid { entity, system, error }
            if *entity == FALLING && system == BODIES && error.field == "mass"),
        "{error}"
    );
    assert_eq!(body(&mut document, FALLING).kind, BodyKind::Dynamic);
    assert!(document.log().is_empty(), "a refused switch was recorded");

    let error = document
        .apply(EditCommand::SetVariant {
            entity: FALLING,
            system: BODIES.to_owned(),
            path: "kind".to_owned(),
            value: Snapshot::Variant {
                name: "Kinematic".into(),
                fields: Vec::new(),
            },
        })
        .expect_err("no mass");
    assert!(matches!(error, EditError::Invalid { .. }), "{error}");
    assert_eq!(body(&mut document, FALLING).kind, BodyKind::Dynamic);
    assert!(document.log().is_empty());
}

/// A switch to a variant the enum does not have is refused by the enum, as
/// a command, and writes nothing.
#[test]
fn a_switch_to_a_variant_the_kind_lacks_is_refused() {
    let mut document = falling();
    let error = document
        .apply(EditCommand::SetVariant {
            entity: FALLING,
            system: BODIES.to_owned(),
            path: "kind".to_owned(),
            value: Snapshot::Variant {
                name: "Floating".into(),
                fields: Vec::new(),
            },
        })
        .expect_err("no such kind");
    assert!(matches!(error, EditError::Path(_)), "{error}");
    assert_eq!(body(&mut document, FALLING).kind, BodyKind::Dynamic);
    assert!(document.log().is_empty());
}

/// **A switch undoes from the history beside the scene**: saved with its
/// history and opened in another document, it is one entry whose undo puts
/// the kind back — what lets the `crcbl scene` CLI undo the inspector's pick.
#[test]
fn a_kind_switch_saved_with_its_history_undoes_in_another_document() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = base.path().join("falling.scn");
    let mut document = falling();
    document.save_as(&dir).expect("a fresh directory");
    let before = document.files().expect("the scene saves");
    switch_kind(&mut document, FALLING, "Static").expect("every kind is a body");
    document
        .save_with_history()
        .expect("a switch is written into the history");

    let mut read =
        Document::open_with_history(&dir, crate::scene::vocabulary()).expect("its own history");
    assert_eq!((read.log().position(), read.log().len()), (1, 1));
    assert_eq!(body(&mut read, FALLING).kind, BodyKind::Static);
    assert!(read.undo().expect("the switch's inverse applies"));
    assert_eq!(body(&mut read, FALLING).kind, BodyKind::Dynamic);
    assert_eq!(read.files().expect("the scene saves"), before);
}
