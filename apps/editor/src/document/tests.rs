//! The document, opened on this build's compiled-in greybox scene.

use super::*;

/// The document every test here opens: this build's compiled-in greybox
/// scene, so nothing below depends on a working directory **or on a game** —
/// `apps/editor/tests/vocabularies.rs` is where the samples' own scenes are
/// opened through this module.
fn document() -> Document {
    crate::scene::built_in_document().expect("the compiled-in scene is a scene")
}

/// The id of the step a test picks, and the `(x, y)` a ray down `-Z` hits it
/// at: `crate::scene`'s three steps, whose centres are these and whose gaps
/// are between them.
const STEPS: [(u32, f64, f64); 3] = [(1, -3.0, 0.25), (2, 0.0, 0.75), (3, 3.0, 1.25)];

/// `(x, y)` in the gap between the first two steps, at the first one's
/// height: outside both, and above the ground slab whose top is `y = 0`.
const GAP: (f64, f64) = (-1.5, 0.25);

/// A command that moves `id` along `axis` by `delta`, read off the
/// document so the *replaced* value is the one actually in the component.
fn nudge(document: &mut Document, id: SceneEntityId, axis: usize, delta: f64) -> EditCommand {
    let (min, max) = document.bounds(id).expect("a block in this document");
    let centre = (min + max) * 0.5;
    let was = f64::from([centre.x, centre.y, centre.z][axis]);
    EditCommand::SetProperty {
        entity: id,
        system: crate::scene::BLOCKS.to_owned(),
        path: format!("position.{axis}"),
        value: Value::Float(was + delta),
    }
}

/// A ray that comes down `-Z` at `(x, y)`, which is how the greybox scene is
/// looked at — every step's `position.2` is 0 and its half extent on that
/// axis is 1.5, so a ray from `z = 20` meets every one of them.
fn ray_at(x: f64, y: f64) -> Ray {
    Ray::new(DVec3::new(x, y, 20.0), DVec3::NEG_Z)
}

/// **The compiled-in scene loads, grouped by the system its chunk came out
/// of, in file order.**
#[test]
fn the_built_in_scene_opens_with_every_block_the_file_names() {
    let mut document = document();
    assert_eq!(document.name(), "greybox");
    let outline = document.outline();
    assert_eq!(outline.len(), 1, "the manifest names one system");
    assert_eq!(outline[0].0, crate::scene::BLOCKS);
    assert_eq!(
        outline[0].1,
        (0..4).map(SceneEntityId).collect::<Vec<_>>(),
        "the outline is not the file's own ids in the file's own order",
    );
    assert_eq!(document.entity_count(), outline[0].1.len());
    assert!(!document.is_dirty(), "a document nobody edited opens clean");
}

/// **The ray hits the entity under the cursor and not its neighbour.**
///
/// Every step is aimed at by name rather than "something was hit": they are
/// 2.4 m wide on a 3.0 m pitch, so a ray 3 m along the row has to come back
/// with the *next* id — and a pick that quietly answered "the nearest entity
/// in the world" would pass a test that only asserted a hit.
#[test]
fn a_ray_picks_the_step_it_points_at_and_not_the_one_beside_it() {
    let mut document = document();
    for (id, x, y) in STEPS {
        // The constant is checked against the file before it is used with
        // it, so a step that moved in `crate::scene` is red here rather than
        // a ray that quietly aims at nothing in particular.
        let (min, max) = document
            .bounds(SceneEntityId(id))
            .unwrap_or_else(|| panic!("step {id} is in the document"));
        let centre = (min + max) * 0.5;
        assert!(
            (f64::from(centre.x) - x).abs() < 1e-5 && (f64::from(centre.y) - y).abs() < 1e-5,
            "step {id} is at {centre:?}, not at ({x}, {y})",
        );
        assert_eq!(
            document.pick(&ray_at(x, y)),
            Some(SceneEntityId(id)),
            "a ray down step {id} picked something else",
        );
    }
}

/// A ray down the **gap between two steps** picks nothing, which is the half
/// that says the pick is a ray and not a nearest-entity query: the same ray
/// 1.5 m either way hits a step.
#[test]
fn a_ray_down_the_gap_between_two_steps_picks_nothing() {
    let mut document = document();
    let (x, y) = GAP;
    assert_eq!(document.pick(&ray_at(x, y)), None);
    assert!(document.pick(&ray_at(x - 1.5, y)).is_some());
    assert!(document.pick(&ray_at(x + 1.5, y)).is_some());
}

/// A ray that points at nothing picks nothing, rather than the nearest
/// thing anywhere.
#[test]
fn a_ray_that_misses_the_scene_picks_nothing() {
    let mut document = document();
    assert!(document.pick(&ray_at(0.0, 400.0)).is_none());
}

/// **The collider follows the edit**, which is what makes a second pick
/// find the entity where it now is rather than where it was.
///
/// The observable is a *pick*, not a field: an edit that wrote the
/// component and skipped [`sync_colliders`] would satisfy every
/// assertion about `position` and still hand the wrong entity back to the
/// next click.
#[test]
fn a_moved_block_is_picked_where_it_now_is() {
    let mut document = document();
    let (_, x, y) = STEPS[0];
    let id = document.pick(&ray_at(x, y)).expect("the first step");

    // Straight up, far enough to clear its own half extent and land
    // somewhere the scene has no other row.
    const LIFT: f64 = 40.0;
    let command = nudge(&mut document, id, 1, LIFT);
    document.apply(command).expect("a block has a y");

    assert!(
        document.pick(&ray_at(x, y)).is_none(),
        "the block still picks where it used to be",
    );
    assert_eq!(
        document.pick(&ray_at(x, y + LIFT)),
        Some(id),
        "and does not pick where it now is",
    );
}

/// **Load → save with no edits is byte-identical.**
///
/// Against the source's own files rather than against a second save: a
/// writer that was merely self-consistent would pass that, and the claim is
/// that opening a scene and saving it changes nothing on disk.
#[test]
fn a_load_and_a_save_with_no_edits_is_byte_identical() {
    let mut document = document();
    let written = document.files().expect("every block has an id");
    let source = crate::scene::built_in_source();
    for (key, text) in &written {
        let committed = source
            .read(Path::new(&format!("{}/{key}", crate::scene::GREYBOX)))
            .unwrap_or_else(|error| panic!("the compiled-in {key}: {error}"));
        assert_eq!(
            text.as_bytes(),
            committed.as_slice(),
            "a load-save round trip changed {key}",
        );
    }
    assert_eq!(
        written.keys().collect::<Vec<_>>(),
        ["env.ron", "scene.ron", "sys/blocks.ron"]
            .iter()
            .collect::<Vec<_>>(),
        "the save wrote a different set of files",
    );
}

/// **Load → edit → save changes exactly the edited bytes.**
///
/// Line by line against the untouched save, and then **component by
/// component within the one line that moved** — so the assertion is not
/// "something changed" but "the `y` of that row changed to that value, and
/// its `x` and `z` did not". A command that wrote a neighbouring axis
/// rewrites the same single line and passes every weaker form of this.
#[test]
fn a_load_edit_and_save_changes_exactly_the_edited_field() {
    let mut before = document();
    let before = before.files().expect("every block has an id");

    let mut document = document();
    let id = SceneEntityId(3);
    document
        .apply(EditCommand::SetProperty {
            entity: id,
            system: crate::scene::BLOCKS.to_owned(),
            path: "position.1".to_owned(),
            value: Value::Float(-3.5),
        })
        .expect("a block has a y");
    let after = document.files().expect("every block has an id");

    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for key in before.keys() {
        if key != "sys/blocks.ron" {
            assert_eq!(before[key], after[key], "{key} moved and nothing edited it");
        }
    }

    let changed: Vec<(usize, &str, &str)> = before["sys/blocks.ron"]
        .lines()
        .zip(after["sys/blocks.ron"].lines())
        .enumerate()
        .filter(|(_, (was, now))| was != now)
        .map(|(line, (was, now))| (line, was, now))
        .collect();
    assert_eq!(
        changed.len(),
        1,
        "one field was edited and {} lines moved: {changed:?}",
        changed.len(),
    );
    let (_, was, now) = changed[0];
    assert!(
        was.trim_start().starts_with("position: ("),
        "the changed line is not a position: {was:?}",
    );
    let (was, now) = (tuple_of(was), tuple_of(now));
    assert_eq!(was.len(), 3, "a position has three components: {was:?}");
    assert_eq!(now[1], "-3.5", "the y is not what the command set it to");
    assert_eq!(
        (was[0].as_str(), was[2].as_str()),
        (now[0].as_str(), now[2].as_str()),
        "editing the y moved the x or the z as well",
    );
}

/// The comma-separated components of the one `(…)` group in a RON line.
fn tuple_of(line: &str) -> Vec<String> {
    let open = line.find('(').expect("a tuple line has an open paren");
    let close = line.rfind(')').expect("and a close one");
    line[open + 1..close]
        .split(',')
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect()
}

/// **An undo restores the file byte for byte**, which is the strongest
/// form of "the inverse restores the value": the RON writer prints a float
/// through Rust's shortest round trip, so a value that came back as a
/// different float prints as a different string.
#[test]
fn undoing_every_edit_restores_the_files_byte_for_byte() {
    let mut document = document();
    let before = document.files().expect("every block has an id");

    for (id, axis, delta) in [(0_u32, 0_usize, 0.35_f64), (1, 1, -0.4), (2, 2, 1.25)] {
        let id = SceneEntityId(id);
        let command = nudge(&mut document, id, axis, delta);
        document.apply(command).expect("a block has that axis");
    }
    assert_ne!(document.files().expect("ids"), before, "nothing was edited");

    while document.undo().expect("every entry names a live entity") {}
    assert_eq!(
        document.files().expect("every block has an id"),
        before,
        "walking the whole log back did not restore the file",
    );
}

/// Redo puts the edits back, and the file matches the edited one exactly.
#[test]
fn redoing_every_undone_edit_restores_the_edited_files() {
    let mut document = document();
    for (id, axis, delta) in [(0_u32, 0_usize, 0.35_f64), (3, 1, -0.4)] {
        let command = nudge(&mut document, SceneEntityId(id), axis, delta);
        document.apply(command).expect("a block has that axis");
    }
    let edited = document.files().expect("every block has an id");

    while document.undo().expect("live entities") {}
    while document.redo().expect("live entities") {}

    assert_eq!(document.files().expect("ids"), edited);
    assert_eq!(document.log().position(), 2);
}

/// **A drag carried on past a save is dirty again**: the save seals the
/// entry it stands on, so the drag's next write starts an entry of its own
/// rather than folding into one the file already holds.
#[test]
fn a_drag_carried_past_a_save_is_dirty_again() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut document = document();
    let drag = document.begin_gesture();
    for x in [0.5, 1.0] {
        let command = nudge(&mut document, SceneEntityId(1), 0, x);
        document.apply_in(command, drag).expect("a block has an x");
    }
    assert_eq!(document.log().len(), 1, "one drag, one entry");
    document
        .save_to(dir.path())
        .expect("the directory is writable");
    assert!(!document.is_dirty());

    let command = nudge(&mut document, SceneEntityId(1), 0, 0.5);
    document.apply_in(command, drag).expect("a block has an x");
    assert!(
        document.is_dirty(),
        "the write after the save was folded into it"
    );
    assert_eq!(document.log().len(), 2);
}

/// **The dirty marker follows the log's position**, in both directions.
///
/// The case a flag gets wrong is the last one: undoing back to where the
/// document was saved is clean again.
#[test]
fn the_dirty_marker_follows_the_logs_position() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut document = document();
    assert!(!document.is_dirty());
    assert!(!document.title().starts_with('*'), "{}", document.title());

    let command = nudge(&mut document, SceneEntityId(0), 0, 0.5);
    document.apply(command).expect("a block has an x");
    assert!(document.is_dirty());
    assert!(
        document.title().starts_with("*greybox"),
        "{}",
        document.title()
    );

    document.save_to(dir.path()).expect("a writable directory");
    assert!(!document.is_dirty(), "a save clears the marker");

    let command = nudge(&mut document, SceneEntityId(0), 0, 0.5);
    document.apply(command).expect("a block has an x");
    assert!(document.is_dirty());
    assert!(document.undo().expect("one entry"));
    assert!(
        !document.is_dirty(),
        "undoing back to the saved position must be clean again",
    );
    assert!(document.redo().expect("one entry"));
    assert!(document.is_dirty(), "and redoing away from it dirty again");
}

/// A save writes the files the scene is, and they read back as the same
/// scene — the round trip through a real directory rather than through
/// text.
#[test]
fn a_saved_directory_opens_again_as_the_same_document() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut document = document();
    let command = nudge(&mut document, SceneEntityId(2), 1, -1.5);
    document.apply(command).expect("a block has a y");
    let expected = document.files().expect("every block has an id");
    document.save_to(dir.path()).expect("a writable directory");

    let mut reopened = Document::open_dir(dir.path(), crate::scene::vocabulary())
        .expect("what we just wrote is a scene");
    assert_eq!(reopened.files().expect("ids"), expected);
    assert_eq!(reopened.origin(), Some(dir.path()));
    assert!(!reopened.is_dirty());
}

/// A command naming an id the document does not hold is refused by that id,
/// and nothing is recorded.
#[test]
fn a_command_for_an_absent_entity_is_refused_and_not_recorded() {
    let mut document = document();
    let error = document
        .apply(EditCommand::SetProperty {
            entity: SceneEntityId(9_999),
            system: crate::scene::BLOCKS.to_owned(),
            path: "position.0".to_owned(),
            value: Value::Float(0.0),
        })
        .expect_err("that id is not in this scene");
    assert!(
        matches!(error, EditError::NoEntity(SceneEntityId(9_999))),
        "{error}",
    );
    assert!(document.log().is_empty());
    assert!(!document.is_dirty());
}

/// A command whose path names nothing is refused, nothing is written, and
/// nothing is recorded — so a typo cannot leave a hole in the history.
#[test]
fn a_command_with_a_bad_path_is_refused_and_not_recorded() {
    let mut document = document();
    let before = document.files().expect("ids");
    let error = document
        .apply(EditCommand::SetProperty {
            entity: SceneEntityId(0),
            system: crate::scene::BLOCKS.to_owned(),
            path: "rotation.0".to_owned(),
            value: Value::Float(1.0),
        })
        .expect_err("a block has no rotation");
    assert!(matches!(error, EditError::Path(_)), "{error}");
    assert!(document.log().is_empty());
    assert_eq!(document.files().expect("ids"), before);
}

/// Selecting an id the document does not hold selects nothing.
#[test]
fn selecting_an_absent_id_selects_nothing() {
    let mut document = document();
    document.select(Some(SceneEntityId(0)));
    assert_eq!(document.primary(), Some(SceneEntityId(0)));
    document.select(Some(SceneEntityId(9_999)));
    assert_eq!(document.primary(), None);
}

/// **The bounds a selection is drawn with come from the component's own
/// `Placement`**, and they are that component's box.
///
/// Checked against the numbers the chunk file spells rather than against
/// whatever the registry answered, so a placement that read the wrong field
/// — or the right field of the wrong row — is red here.
#[test]
fn the_bounds_of_a_block_are_its_centre_plus_and_minus_its_half_extents() {
    let mut document = document();
    let (min, max) = document.bounds(SceneEntityId(3)).expect("the third step");
    let centre = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    assert!((f64::from(centre.x) - 3.0).abs() < 1e-5, "{centre:?}");
    assert!((f64::from(centre.y) - 1.25).abs() < 1e-5, "{centre:?}");
    assert!((f64::from(half.x) - 1.2).abs() < 1e-5, "{half:?}");
    assert!((f64::from(half.y) - 1.25).abs() < 1e-5, "{half:?}");
    assert!(document.bounds(SceneEntityId(9_999)).is_none());
}
