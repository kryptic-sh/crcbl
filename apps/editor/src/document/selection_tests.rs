//! The selection set: its order and primary, entities dropping out of it as
//! they leave the document, the pivot, and a selection's delete, duplicate
//! and copy each one entry.

use super::*;

fn document() -> Document {
    crate::scene::built_in_document().expect("the compiled-in scene is a scene")
}

/// **The selection keeps the order entities joined it, and the last is the
/// primary**: a toggle adds one as the primary or takes it out — handing the
/// role back to the one before — a plain select replaces the lot, and ids the
/// document does not hold, or holds twice over, never get in.
#[test]
fn a_selection_keeps_its_order_and_the_last_is_the_primary() {
    let mut document = document();
    let [a, b, c] = [1, 3, 2].map(SceneEntityId);
    document.select(Some(a));
    document.toggle_selected(b);
    document.toggle_selected(c);
    assert_eq!(document.selection(), [a, b, c]);
    assert_eq!(document.primary(), Some(c));
    assert!(document.is_selected(b));

    document.toggle_selected(c);
    assert_eq!(
        document.selection(),
        [a, b],
        "the toggle did not take it out"
    );
    assert_eq!(document.primary(), Some(b), "the role did not pass back");
    document.toggle_selected(a);
    assert_eq!(document.selection(), [b], "a toggle of a middle one");

    document.toggle_selected(SceneEntityId(9_999));
    assert_eq!(document.selection(), [b], "an absent id got in");

    document.select(Some(a));
    assert_eq!(document.selection(), [a], "a plain select did not replace");

    document.set_selection([c, SceneEntityId(9_999), a, c]);
    assert_eq!(document.selection(), [c, a]);
    assert_eq!(document.primary(), Some(a));

    document.select(None);
    assert!(document.selection().is_empty());
    assert_eq!(document.primary(), None);
}

/// **An entity that stops existing drops out of the selection, and the rest
/// keep their order**: a delete, an undo of a duplicate, and nothing brought
/// back by an undo is selected again — the selection is not in the log.
#[test]
fn entities_that_stop_existing_drop_out_of_the_selection() {
    let mut document = document();
    let [a, b, c] = [1, 2, 3].map(SceneEntityId);
    document.set_selection([a, b, c]);
    document.delete(&[b]).expect("held");
    assert_eq!(document.selection(), [a, c]);

    assert!(document.undo().expect("the delete's inverse applies"));
    assert_eq!(
        document.selection(),
        [a, c],
        "an undo selected what it restored"
    );

    let copies = document.duplicate(&[a]).expect("held");
    document.set_selection([copies[0], c]);
    assert!(document.undo().expect("the duplicate's inverse applies"));
    assert_eq!(
        document.selection(),
        [c],
        "the undone copy is still selected"
    );
}

/// **The pivot is the centre of the box around every selected entity's
/// box**: the first and third steps reach from `x = -4.2` to `4.2` and from
/// the ground to `y = 2.5`, so it is `(0, 1.25, 0)` — not the mean of their
/// centres, `(0, 0.75, 0)`, nor either one's centre. One entity's pivot is
/// its own centre, and an empty selection has none.
#[test]
fn the_pivot_is_the_centre_of_the_selections_bounds() {
    let mut document = document();
    assert_eq!(document.selection_pivot(), None, "nothing selected");

    document.set_selection([SceneEntityId(1), SceneEntityId(3)]);
    let pivot = document.selection_pivot().expect("two placed steps");
    assert!(
        pivot.abs_diff_eq(DVec3::new(0.0, 1.25, 0.0), 1e-12),
        "{pivot:?}"
    );

    document.select(Some(SceneEntityId(3)));
    let pivot = document.selection_pivot().expect("a placed step");
    assert!(
        pivot.abs_diff_eq(DVec3::new(3.0, 1.25, 0.0), 1e-12),
        "{pivot:?}"
    );
}

/// **A selection's delete, duplicate and copy-and-paste are one entry
/// each**, every entity in it taken, copied or pasted, and one undo puts the
/// files back byte for byte.
#[test]
fn a_selections_delete_duplicate_and_paste_are_one_entry_each() {
    let mut document = document();
    let before = document.files().expect("ids");
    let picked = [SceneEntityId(1), SceneEntityId(3)];

    document.delete(&picked).expect("held");
    assert_eq!(document.entity_count(), 2);
    assert_eq!(document.log().len(), 1, "a delete of two is one entry");
    assert!(document.undo().expect("the batch's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);

    let copies = document.duplicate(&picked).expect("held");
    assert_eq!(copies, [SceneEntityId(4), SceneEntityId(5)]);
    for (copy, original) in copies.into_iter().zip(picked) {
        assert_eq!(
            document
                .read(copy, crate::scene::BLOCKS, "position.0")
                .expect("a copied block"),
            document
                .read(original, crate::scene::BLOCKS, "position.0")
                .expect("a held block"),
        );
    }
    assert_eq!(
        document.log().position(),
        1,
        "a duplicate of two is one entry"
    );
    assert!(document.undo().expect("the batch's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);

    let text = document.copy(&picked).expect("held");
    let pasted = document.paste(&text).expect("its own copy");
    assert_eq!(pasted.len(), 2, "the copy did not carry both");
    assert_eq!(document.log().position(), 1, "a paste of two is one entry");
    assert!(document.undo().expect("the batch's inverse applies"));
    assert_eq!(document.files().expect("ids"), before);
}
