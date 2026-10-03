//! **The dogfood pass** — `docs/plan/08-editor.md`'s task 8, and
//! `docs/plan/sample/07-towers.md`'s milestone 2: towers' whole field
//! authored from an empty scene through the editor's own entry points, saved
//! by the editor's save-as, and then byte for byte the committed
//! `apps/towers/assets/scenes/field.scn/` — which towers loads and plays.
//!
//! Every step is a key, a click or typed text delivered through the headless
//! shell, as `exit_criterion` drives its scene: the environment pasted into
//! the scene's inspector rows; each corner of the path and each plot added
//! from the inspector's add-an-entity buttons or duplicated with Ctrl+D, its
//! numbers pasted into its rows and its label typed; the directory typed on
//! the save-as line. **No file is written by anything but the editor's
//! save**, and no scene text or RON is written by hand: what the clipboard
//! carries is one number at a time, as a person copies it from anywhere.
//!
//! **Numbers are pasted rather than dragged**: a drag-value takes no typed
//! number — it moves `step` a pixel — so the clipboard is the one way a number
//! field takes exactly the value a person means (`docs/backlog.md`, the pass's
//! finding).

use super::*;

use std::collections::BTreeMap;
use std::path::Path;

use crcbl::assets::DirSource;
use crcbl::shell::ClipboardOffer;
use crcbl_towers::tower::Kind;
use crcbl_towers::{Controls, DEFAULT_TICK_HZ, Game, Map, WAVES};

use super::files::{chord, type_and_enter};
use super::scene_inspector::{entity_add, environment_field};
use crate::document::origin_tests::tree;

/// The frames the editor may run: enough for every click, paste and key.
const FRAMES: u64 = 4_000;

/// Towers' path: one waypoint per corner.
const WAYPOINTS: &str = "waypoints";

/// Towers' build plots.
const PLOTS: &str = "plots";

/// The environment's rows in the inspector, and what the author enters on
/// each, axis by axis — the camera towers' field is viewed from and the
/// light it sits in.
const ENVIRONMENT: [(&str, [&str; 3]); 3] = [
    ("camera", ["0.0", "32.0", "23.0"]),
    ("look_at", ["0.0", "0.0", "0.0"]),
    ("ambient", ["0.17", "0.19", "0.22"]),
];

/// The corners of the path the author enters, in walking order: each one's
/// `x` and `z`. Every corner stands on the ground, which a new waypoint
/// already does.
const PATH: [(&str, &str); 4] = [
    ("-14.0", "8.0"),
    ("8.0", "8.0"),
    ("8.0", "-6.0"),
    ("-10.0", "-6.0"),
];

/// The build plots the author enters, in the order towers numbers them: each
/// one's label, `x` and `z`.
const PLOT_TABLE: [(&str, &str, &str); 5] = [
    ("entry", "-6.0", "3.0"),
    ("bend", "3.0", "3.0"),
    ("east", "13.0", "1.0"),
    ("middle", "2.0", "-1.0"),
    ("gate", "-7.0", "-1.0"),
];

/// A waypoint's rows: its order, then its position.
const ORDER_ROW: usize = 0;
/// A waypoint's or a plot's position row.
const POSITION_ROW: usize = 1;
/// A plot's rows: its label, then its position.
const LABEL_ROW: usize = 0;

/// The axes of a position the author enters: `x` and `z`.
const X: usize = 0;
/// See [`X`].
const Z: usize = 2;

/// Where a test-built tower stands for the first wave: the two plots that
/// cover the opening leg, as towers' own scripted run builds them.
const DEFENDED: [u8; 2] = [0, 1];

/// The middle of the widget of the `row`th row of the inspector block
/// `fields` — a leaf row's, or with `axis` that axis of a vector row's: a row
/// is a label and then its widget, or one cell per axis, each a label and
/// then its widget.
fn widget(
    editor: &Editor<HeadlessShell>,
    fields: NodeKey,
    row: usize,
    axis: Option<usize>,
) -> PhysicalPoint {
    let ui = editor.panels.ui();
    let row = ui.child_keys(fields)[row];
    let key = match axis {
        None => ui.child_keys(row)[1],
        Some(axis) => ui.child_keys(ui.child_keys(row)[1 + axis])[1],
    };
    centre(editor, key)
}

/// The rows of the selection's one section.
fn section(editor: &Editor<HeadlessShell>) -> NodeKey {
    editor
        .panels
        .section_fields(0)
        .expect("the inspector drew the selection's section")
}

/// What a person does to put an exact number in a field: clicks it, copies
/// `text` from wherever it was, and presses Ctrl+V — then the frames the
/// clipboard takes to answer.
fn paste_into(editor: &mut Editor<HeadlessShell>, at: PhysicalPoint, text: &str) {
    click(editor, at);
    let window = editor.window;
    editor
        .shell_mut()
        .clipboard_offer(window, &[ClipboardOffer::text(text)])
        .expect("the headless clipboard takes an offer");
    chord(editor, Modifiers::CTRL, KeyCode::KeyV);
    for _ in 0..3 {
        editor.frame().expect("a frame");
    }
}

/// Clicks `at`, a text field, selects what it holds and types `text` over
/// it.
fn type_into(editor: &mut Editor<HeadlessShell>, at: PhysicalPoint, text: &str) {
    click(editor, at);
    assert!(
        editor.panels.text_editing(),
        "the click engaged no text field"
    );
    chord(editor, Modifiers::CTRL, KeyCode::KeyA);
    let window = editor.window;
    editor
        .shell_mut()
        .commit_text(window, text)
        .expect("the headless shell takes text");
    editor.frame().expect("a frame");
}

/// A pixel of the viewport no entity is drawn in: just under its top edge,
/// where the camera looks at sky.
fn sky(editor: &Editor<HeadlessShell>) -> PhysicalPoint {
    let (min, max) = editor.panels.viewport_pixels();
    PhysicalPoint {
        x: f64::from((min.x + max.x) * 0.5),
        y: f64::from(min.y + 4.0),
    }
}

/// The float `text` spells, as the leaf it was pasted into holds it.
fn float(text: &str) -> Value {
    Value::Float(text.parse().expect("the table spells a number"))
}

/// `files` as text, so a failed comparison prints lines rather than bytes.
fn as_text(files: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    files
        .into_iter()
        .map(|(key, bytes)| (key, String::from_utf8(bytes).expect("scene text is UTF-8")))
        .collect()
}

/// Ticks `game` `ticks` times with no command.
fn run(game: &mut Game, ticks: u32) {
    for _ in 0..ticks {
        game.tick();
    }
}

/// Ticks `game` once, sending `controls`.
fn send(game: &mut Game, controls: Controls) {
    game.set_controls(controls);
    game.tick();
}

/// How many ticks the first wave takes to be released and walk the whole
/// path at its slowest creep's pace, at towers' rate — with a second's slack.
fn first_wave_ticks(map: &Map) -> u32 {
    let first = WAVES[0];
    let slowest = (0..first.creeps())
        .filter_map(|index| first.kind_at(index))
        .map(|kind| kind.spec().speed)
        .fold(f64::INFINITY, f64::min);
    let span: f64 = (0..first.creeps())
        .filter_map(|index| first.gap_after(index))
        .sum();
    let seconds = span + map.path().length() / slowest + 1.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ticks = (seconds * f64::from(DEFAULT_TICK_HZ)).ceil() as u32;
    ticks
}

/// **Towers' field, authored from empty in the editor, is the committed field
/// byte for byte, and towers plays it.** The steps, each asserted as it
/// lands:
///
/// 1. Ctrl+N: nothing in the scene, nothing selected.
/// 2. The environment: each camera, aim and ambient number pasted into the
///    scene's inspector rows.
/// 3. The path: the first corner from the inspector's `+ waypoints`, each
///    next one Ctrl+D of the last; each one's order and `x` and `z` pasted.
/// 4. A click on the sky, selecting nothing; then the plots: the first from
///    `+ plots`, each next one Ctrl+D of the last; each one's label typed and
///    its `x` and `z` pasted. The scene now has no problem a save would
///    report.
/// 5. The toolbar's Save as, a directory named `field.scn` typed, Enter: the
///    scene is that directory's, and clean.
/// 6. That directory, read off the disk, is the committed field's files —
///    every key and every byte.
/// 7. [`Map::load`] reads it as the map towers ships, and a game on it holds
///    the first wave with two towers, as towers' own scripted run does.
#[test]
fn towers_field_authored_from_empty_is_the_committed_field_and_plays() {
    let out = tempfile::tempdir().expect("a temporary directory");
    let saved = out.path().join("field.scn");
    let mut editor = headless(FRAMES);
    editor.frame().expect("a frame");

    // 1. A scene from empty.
    chord(&mut editor, Modifiers::CTRL, KeyCode::KeyN);
    assert_eq!(editor.document().entity_count(), 0, "1: not empty");
    assert_eq!(editor.document().primary(), None, "1: something selected");

    // 2. The environment.
    for (row, (field, values)) in ENVIRONMENT.iter().enumerate() {
        for (axis, text) in values.iter().enumerate() {
            let at = environment_field(&mut editor, row, axis);
            paste_into(&mut editor, at, text);
            let path = format!("{field}.{axis}");
            assert_eq!(
                editor.document().read_environment(&path).expect("a leaf"),
                Value::Float(f64::from(text.parse::<f32>().expect("a number"))),
                "2: `{path}` did not take {text}",
            );
        }
    }

    // 3. The path.
    let add = entity_add(&editor, WAYPOINTS);
    click(&mut editor, add);
    for (order, (x, z)) in (0..).zip(PATH) {
        if order > 0 {
            chord(&mut editor, Modifiers::CTRL, KeyCode::KeyD);
        }
        let id = editor.document().primary().expect("3: a waypoint selected");
        assert_eq!(id, SceneEntityId(order), "3: not the next id");
        if order > 0 {
            let at = widget(&editor, section(&editor), ORDER_ROW, None);
            paste_into(&mut editor, at, &order.to_string());
        }
        let at = widget(&editor, section(&editor), POSITION_ROW, Some(X));
        paste_into(&mut editor, at, x);
        let at = widget(&editor, section(&editor), POSITION_ROW, Some(Z));
        paste_into(&mut editor, at, z);
        for (path, value) in [
            ("order", Value::UInt(u64::from(order))),
            ("position.0", float(x)),
            ("position.1", float("0.0")),
            ("position.2", float(z)),
        ] {
            assert_eq!(
                editor
                    .document_mut()
                    .read(id, WAYPOINTS, path)
                    .expect("a leaf"),
                value,
                "3: waypoint #{id}'s `{path}`",
            );
        }
    }

    // 4. The plots.
    let at = sky(&editor);
    click(&mut editor, at);
    assert_eq!(editor.document().primary(), None, "4: the sky selected");
    let add = entity_add(&editor, PLOTS);
    click(&mut editor, add);
    for (index, (label, x, z)) in (0..).zip(PLOT_TABLE) {
        if index > 0 {
            chord(&mut editor, Modifiers::CTRL, KeyCode::KeyD);
        }
        let id = editor.document().primary().expect("4: a plot selected");
        assert_eq!(
            id,
            SceneEntityId(PATH.len() as u32 + index),
            "4: not the next id"
        );
        let at = widget(&editor, section(&editor), LABEL_ROW, None);
        type_into(&mut editor, at, label);
        let at = widget(&editor, section(&editor), POSITION_ROW, Some(X));
        paste_into(&mut editor, at, x);
        let at = widget(&editor, section(&editor), POSITION_ROW, Some(Z));
        paste_into(&mut editor, at, z);
        for (path, value) in [
            ("label", Value::Text(label.to_owned())),
            ("position.0", float(x)),
            ("position.1", float("0.0")),
            ("position.2", float(z)),
        ] {
            assert_eq!(
                editor.document_mut().read(id, PLOTS, path).expect("a leaf"),
                value,
                "4: plot #{id}'s `{path}`",
            );
        }
    }
    assert_eq!(
        editor.document_mut().problems().expect("ids"),
        Vec::<String>::new(),
        "4: the field has problems",
    );

    // 5. Saved by the editor, into a directory typed on the save-as line.
    let [_, _, save_as] = editor.panels.file_buttons();
    let at = centre(&editor, save_as);
    click(&mut editor, at);
    type_and_enter(&mut editor, &saved.display().to_string());
    assert_eq!(
        editor.document().origin(),
        Some(saved.as_path()),
        "5: not its own"
    );
    assert!(!editor.document().is_dirty(), "5: still dirty");
    editor.finish(ExitReason::FrameBudget).expect("teardown");

    // 6. Byte for byte the committed field.
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../towers/assets/scenes/field.scn");
    let shipped = as_text(tree(&committed));
    assert!(
        !shipped.is_empty(),
        "6: no committed field at {}",
        committed.display()
    );
    assert_eq!(as_text(tree(&saved)), shipped, "6: not the committed field");

    // 7. Towers loads it, and plays it.
    let map = Map::load(&DirSource::at(saved.clone()), Path::new("")).expect("7: a map");
    assert_eq!(map, Map::built_in(), "7: not the map towers ships");
    let mut game = Game::new(DEFAULT_TICK_HZ, &map).expect("7: a game on the authored map");
    for plot in DEFENDED {
        send(
            &mut game,
            Controls {
                place: Some(plot),
                kind: Kind::Bolt,
                ..Controls::default()
            },
        );
    }
    send(
        &mut game,
        Controls {
            start_wave: true,
            ..Controls::default()
        },
    );
    run(&mut game, first_wave_ticks(&map));
    let stats = game.stats();
    assert_eq!(stats.towers, DEFENDED.len(), "7: the towers were not built");
    assert_eq!(stats.refused, 0, "7: a command was refused");
    assert!(
        stats.kills >= u64::from(WAVES[0].creeps()),
        "7: two towers killed {} of the first wave's {}",
        stats.kills,
        WAVES[0].creeps(),
    );
    assert_eq!(stats.leaks, 0, "7: the first wave leaked");
}
