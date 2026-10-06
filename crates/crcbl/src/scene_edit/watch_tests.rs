//! The document's chunk files watched: a changed chunk is due until taken,
//! and the watch follows the document's directory and manifest —
//! `scene_edit::watch`'s module docs hold the decisions.

use std::time::Duration;

use crate::assets::watch::{POLL_INTERVAL, SETTLE};

use super::reload_tests::{BLOCKS_RON, PADS, opened, write_blocks};
use super::tests::BLOCKS;
use super::*;

/// Polls `watch` over `document` once a [`POLL_INTERVAL`] from `*now` until
/// [`SETTLE`] and two more looks have passed, advancing `*now`, and hands back
/// every system the looks offered.
fn poll_past_settle(
    watch: &mut ChunkWatch,
    document: &Document,
    now: &mut Duration,
) -> Vec<String> {
    let mut offered = Vec::new();
    let until = *now + SETTLE + POLL_INTERVAL * 2;
    while *now < until {
        *now += POLL_INTERVAL;
        offered.extend(watch.poll(document, *now));
    }
    offered
}

/// **A chunk another program changed is offered once and is due until it is
/// taken**; the chunk nobody touched is not.
#[test]
fn a_changed_chunk_is_offered_once_and_due_until_taken() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, document) = opened(&base);
    let mut now = Duration::ZERO;
    let mut watch = ChunkWatch::new(&document, now);
    assert!(poll_past_settle(&mut watch, &document, &mut now).is_empty());

    write_blocks(
        &dir,
        &BLOCKS_RON.replace("(1.0, 0.0, 0.0)", "(14.0, 0.0, 0.0)"),
    );
    assert_eq!(poll_past_settle(&mut watch, &document, &mut now), [BLOCKS]);
    assert!(poll_past_settle(&mut watch, &document, &mut now).is_empty());
    assert_eq!(watch.take_due(), [BLOCKS], "the change was not held");
    assert!(watch.take_due().is_empty());
}

/// **The watch follows the document**: a document saved into another
/// directory drops what was due in the old one and watches the new one, and
/// a system unlisted from the manifest is no longer watched or due.
#[test]
fn the_watch_follows_the_documents_directory_and_manifest() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let (dir, mut document) = opened(&base);
    let mut now = Duration::ZERO;
    let mut watch = ChunkWatch::new(&document, now);

    write_blocks(
        &dir,
        &BLOCKS_RON.replace("(1.0, 0.0, 0.0)", "(14.0, 0.0, 0.0)"),
    );
    assert_eq!(poll_past_settle(&mut watch, &document, &mut now), [BLOCKS]);
    let moved = base.path().join("moved.scn");
    document.save_as(&moved).expect("a fresh directory");
    assert!(poll_past_settle(&mut watch, &document, &mut now).is_empty());
    assert!(
        watch.take_due().is_empty(),
        "a change in the old directory is still due"
    );

    write_blocks(&dir, BLOCKS_RON);
    assert!(
        poll_past_settle(&mut watch, &document, &mut now).is_empty(),
        "the old directory is still watched"
    );
    write_blocks(
        &moved,
        &BLOCKS_RON.replace("(0.0, 0.0, 0.0)", "(13.0, 0.0, 0.0)"),
    );
    assert_eq!(poll_past_settle(&mut watch, &document, &mut now), [BLOCKS]);

    std::fs::write(moved.join("sys/pads.ron"), "changed").expect("written");
    assert_eq!(poll_past_settle(&mut watch, &document, &mut now), [PADS]);
    // Every pad gone, so the system can be unlisted.
    document
        .delete(&[SceneEntityId(1), SceneEntityId(2)])
        .expect("the pads go");
    document
        .apply(EditCommand::UnlistSystem {
            system: PADS.to_owned(),
        })
        .expect("an empty system unlists");
    assert!(poll_past_settle(&mut watch, &document, &mut now).is_empty());
    assert_eq!(
        watch.take_due(),
        [BLOCKS],
        "an unlisted system is still due, or a listed one's change went"
    );
}
