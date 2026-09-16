//! The console panel on the element tree: where its parts land, what a frame
//! draws, what the pointer does to it, and what its field gained by becoming
//! [`Ui::text_input`](crate::tree::Ui::text_input).

use core::time::Duration;

use crcbl_core::log::Level;
use crcbl_core::log::console::Record;

use super::*;
use crate::draw_list::{ClipRect, DrawCommand};
use crate::edit::{ClipboardOp, Motion};
use crate::tree::{ClipboardAnswer, ClipboardReply, DOUBLE_CLICK_TIME};

/// The five extents every "it is on screen" test in this repository uses.
const EXTENTS: [(u32, u32); 5] = [
    (960, 720),
    (800, 600),
    (1920, 1080),
    (1440, 400),
    (600, 900),
];

/// Every frame's length unless a test says otherwise, as the tree's own text
/// input tests use it: the caret's blink and a double-click are timed by it.
const FRAME: Duration = Duration::from_millis(16);

/// What `default.css` draws the console's caret in, as
/// `text-input.console-input > .text-input-caret` names it.
const CARET_COLOUR: [f32; 4] = [0.94, 0.95, 1.0, 1.0];

fn atlas() -> FontAtlas {
    FontAtlas::built_in()
}

/// The pointer away from everything the console draws.
fn idle() -> PointerInput {
    PointerInput::hovering(Vec2::splat(-1.0))
}

fn press_at(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: true,
        released: false,
    }
}

fn release_at(pos: Vec2) -> PointerInput {
    PointerInput {
        pos,
        down: false,
        released: true,
    }
}

/// A frame's text input of `edits`.
fn typed(edits: impl IntoIterator<Item = Edit>) -> TextInput {
    TextInput {
        dt: FRAME,
        edits: edits.into_iter().collect(),
        clipboard: Vec::new(),
    }
}

/// A frame's text input with no edits.
fn quiet() -> TextInput {
    typed([])
}

fn insert(text: &str) -> Edit {
    Edit::Insert(text.to_owned())
}

/// One frame: lay the panel out with `pointer` and `input`, and answer with
/// where everything landed.
fn frame(
    panel: &mut ConsolePanel,
    extent: (u32, u32),
    pointer: PointerInput,
    input: TextInput,
) -> ConsoleLayout {
    panel.layout(extent, &atlas(), pointer, input)
}

/// One quiet frame at `extent`, with the pointer off the panel.
fn still(panel: &mut ConsolePanel, extent: (u32, u32)) -> ConsoleLayout {
    frame(panel, extent, idle(), quiet())
}

/// A panel with `lines` info records in its log and `typed` in its field.
///
/// The line is set rather than typed, because a test about where a box lands
/// has nothing to say about how the text got there.
fn panel(lines: &[&str], text: &str) -> ConsolePanel {
    let mut panel = ConsolePanel::new();
    let records: Vec<Record> = lines
        .iter()
        .enumerate()
        .map(|(index, message)| Record {
            sequence: index as u64,
            level: Level::Info,
            target: crcbl_core::log::console::CONSOLE_TARGET.to_owned(),
            message: (*message).to_owned(),
            elapsed: Duration::ZERO,
        })
        .collect();
    panel.log_mut().push_records(&records);
    panel.set_line(text);
    panel
}

/// Every text command a frame draws, in order.
fn texts(dl: &DrawList) -> Vec<String> {
    dl.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Every text command a frame draws, with where it landed and its colour.
fn placed(dl: &DrawList) -> Vec<(Vec2, String, [f32; 4])> {
    dl.commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Text {
                pos, text, color, ..
            } => Some((*pos, text.clone(), *color)),
            _ => None,
        })
        .collect()
}

/// One frame drawn, after one quiet frame to place it.
fn drawn(panel: &mut ConsolePanel, extent: (u32, u32)) -> (ConsoleLayout, DrawList) {
    let layout = still(panel, extent);
    let mut dl = DrawList::new();
    panel.render(&mut dl, &layout, &atlas());
    (layout, dl)
}

/// The centre of the first key on the laid-out keyboard whose cap is `cap`.
fn key_centre(layout: &ConsoleLayout, cap: KeyCap) -> Vec2 {
    let key = layout
        .keyboard()
        .keys()
        .iter()
        .find(|key| key.cap == cap)
        .unwrap_or_else(|| panic!("the keyboard has no {cap:?} key"));
    (key.min + key.max) * 0.5
}

// ---------------------------------------------------------------------------
// The field, which is now the tree's text input
// ---------------------------------------------------------------------------

/// **The first frame the panel is laid out takes the edits it is handed.**
///
/// A console prompt is engaged from the moment the panel exists —
/// [`ConsolePanel::new`] builds the tree once for the field's identity and
/// engages it — because a shell batch can carry a whole line and its `Enter`,
/// and a frame of warm-up would submit an empty line. An input engaged on the
/// frame the engagement began takes no edits, so nothing here would work
/// without that constructor build.
#[test]
fn the_first_laid_out_frame_takes_the_edits_it_is_handed() {
    let mut panel = ConsolePanel::new();
    frame(&mut panel, (960, 720), idle(), typed([insert("q")]));
    assert_eq!(
        panel.line(),
        "q",
        "the first frame swallowed what was typed on it",
    );
}

/// **A tapped key is not lost while the field is not taking edits.**
///
/// [`ConsolePanel::edit`] queues, so an edit that arrives on a frame the tree
/// reports the field un-engaged — a frame a click elsewhere in the panel
/// committed it — is handed over on the next frame that takes any, in order.
#[test]
fn a_queued_edit_waits_for_a_frame_the_field_takes_edits_on() {
    let mut panel = panel(&[], "");
    // A click on Send commits the field, so that frame's edits are not taken.
    let layout = still(&mut panel, (960, 720));
    let on_button = (layout.send().0 + layout.send().1) * 0.5;
    frame(&mut panel, (960, 720), press_at(on_button), quiet());
    frame(
        &mut panel,
        (960, 720),
        release_at(on_button),
        typed([insert("a"), insert("b")]),
    );
    still(&mut panel, (960, 720));
    assert_eq!(
        panel.line(),
        "ab",
        "the edits that arrived while the field was committed were dropped",
    );
}

/// **The field selects, copies and pastes**, which is the whole of what moving
/// it onto [`Ui::text_input`](crate::tree::Ui::text_input) bought: the console
/// had none of it.
#[test]
fn the_field_selects_copies_and_pastes() {
    let extent = (960, 720);
    let mut panel = ConsolePanel::new();

    frame(&mut panel, extent, idle(), typed([insert("r_ao_view 1")]));
    assert_eq!(panel.line(), "r_ao_view 1");

    // Select the whole line and copy it: the clipboard is asked to take it.
    frame(
        &mut panel,
        extent,
        idle(),
        typed([Edit::SelectAll, Edit::Copy]),
    );
    let requests = panel.take_clipboard_requests();
    assert_eq!(
        requests.len(),
        1,
        "the copy made {} clipboard requests",
        requests.len(),
    );
    assert_eq!(
        requests[0].op,
        ClipboardOp::Offer("r_ao_view 1".to_owned()),
        "the copy offered something other than the selected line",
    );
    let field = requests[0].from;

    // A word-selecting move, then a paste over the selection: the value the
    // answer carries replaces the word and nothing else.
    frame(
        &mut panel,
        extent,
        idle(),
        typed([
            Edit::Move {
                motion: Motion::End,
                select: false,
            },
            Edit::Move {
                motion: Motion::WordLeft,
                select: true,
            },
            Edit::Paste,
        ]),
    );
    let asked = panel.take_clipboard_requests();
    assert_eq!(
        asked.iter().map(|ask| &ask.op).collect::<Vec<_>>(),
        [&ClipboardOp::Read],
        "the paste did not ask the clipboard to read",
    );

    frame(
        &mut panel,
        extent,
        idle(),
        TextInput {
            dt: FRAME,
            edits: Vec::new(),
            clipboard: vec![ClipboardAnswer {
                to: field,
                reply: ClipboardReply::Text("0".to_owned()),
            }],
        },
    );
    assert_eq!(
        panel.line(),
        "r_ao_view 0",
        "the pasted text did not replace the selected word",
    );
}

/// **A double-click selects the word under it, and a drag extends the
/// selection** — through the panel's own pointer, because the field is
/// hit-tested by the tree and not by a rectangle the panel keeps.
///
/// What is asserted is the *effect* of the selection rather than a highlight
/// rectangle: typing over a selection replaces it, so a word selected and then
/// typed over is a word gone.
#[test]
fn a_double_click_selects_a_word_and_a_drag_extends_it() {
    let extent = (960, 720);
    let mut panel = ConsolePanel::new();
    frame(&mut panel, extent, idle(), typed([insert("alpha beta")]));

    let layout = still(&mut panel, extent);
    let advance = layout.style().advance(&atlas());
    let at = |column: f32| {
        Vec2::new(
            layout.text_pos().x + column * advance,
            layout.text_pos().y + layout.style().row_height() * 0.5,
        )
    };

    // Two presses inside `DOUBLE_CLICK_TIME` on the same spot: the second
    // selects the word under it.
    let on_beta = at(7.5);
    frame(&mut panel, extent, press_at(on_beta), quiet());
    frame(&mut panel, extent, release_at(on_beta), quiet());
    assert!(
        FRAME * 2 < DOUBLE_CLICK_TIME,
        "the two presses are too far apart to be a double-click",
    );
    frame(&mut panel, extent, press_at(on_beta), quiet());
    frame(&mut panel, extent, release_at(on_beta), quiet());
    frame(&mut panel, extent, idle(), typed([insert("gamma")]));
    assert_eq!(
        panel.line(),
        "alpha gamma",
        "the double-click did not select the word under it",
    );

    // A press and a drag: the selection follows the pointer, so what is typed
    // replaces everything between where the press began and where it ended.
    panel.set_line("alpha gamma");
    still(&mut panel, extent);
    frame(&mut panel, extent, press_at(at(0.0)), quiet());
    frame(&mut panel, extent, press_at(at(6.0)), quiet());
    frame(&mut panel, extent, release_at(at(6.0)), quiet());
    frame(&mut panel, extent, idle(), typed([insert("x")]));
    assert_eq!(
        panel.line(),
        "xgamma",
        "the drag did not extend the selection over the word it crossed",
    );
}

// ---------------------------------------------------------------------------
// The scale probe against the tree
// ---------------------------------------------------------------------------

/// **The scale the probe chose is the scale the tree delivers.**
///
/// [`fits`] is arithmetic about a tree it never lays out — there is one build a
/// frame — so this is what stops it drifting from [`build`]'s box model: at a
/// wide spread of extents, the chosen scale really does show the floors, and
/// the next scale up really would not.
#[test]
fn the_scale_the_probe_chose_is_the_scale_the_tree_delivers() {
    let atlas = atlas();
    let spread: Vec<(u32, u32)> = EXTENTS
        .into_iter()
        .chain([
            (640, 480),
            (1280, 720),
            (2560, 1440),
            (3840, 2160),
            (1024, 300),
            (400, 1200),
            (1920, 500),
            (720, 1280),
        ])
        .collect();

    for extent in spread {
        let mut content = panel(&["one"], "");
        let chosen = still(&mut content, extent).style().scale as u32;

        // What the tree actually delivered at the scale the probe chose.
        let mut at_chosen = panel(&["one"], "");
        let style = ConsoleStyle::pixel_art(chosen);
        let layout = at_chosen.layout_with(extent, &atlas, &style, idle(), quiet());
        let delivered_rows = layout.log_rows();
        let delivered_columns = layout.field_columns(&atlas);

        if chosen > 1 {
            assert!(
                delivered_rows >= MINIMUM_LOG_ROWS && delivered_columns >= MINIMUM_FIELD_COLUMNS,
                "{extent:?}: the probe chose scale {chosen} and the tree gave \
                 {delivered_rows} rows and {delivered_columns} columns",
            );
        }

        // **Figure for figure, at every scale** — not just either side of the
        // floors: a probe that agreed about whether a panel fits while
        // disagreeing about how much of it fits is a probe that will choose
        // wrongly at the next extent nobody tested.
        let size = Vec2::new(extent.0 as f32, extent.1 as f32);
        for scale in 1..=MenuStyle::MAX_SCALE {
            let style = ConsoleStyle::pixel_art(scale);
            let mut at_scale = panel(&["one"], "");
            let laid = at_scale.layout_with(extent, &atlas, &style, idle(), quiet());
            assert_eq!(
                probe(size, &atlas, &style),
                (laid.log_rows(), laid.field_columns(&atlas)),
                "{extent:?} at scale {scale}: the probe's arithmetic and the tree \
                 disagree",
            );
        }

        // And the scale above the one it chose is one the tree would refuse.
        if chosen < MenuStyle::MAX_SCALE {
            let bigger = ConsoleStyle::pixel_art(chosen + 1);
            let mut at_bigger = panel(&["one"], "");
            let taller = at_bigger.layout_with(extent, &atlas, &bigger, idle(), quiet());
            assert!(
                taller.log_rows() < MINIMUM_LOG_ROWS
                    || taller.field_columns(&atlas) < MINIMUM_FIELD_COLUMNS,
                "{extent:?}: scale {} shows {} rows and {} columns, so the probe \
                 stopped one scale early",
                chosen + 1,
                taller.log_rows(),
                taller.field_columns(&atlas),
            );
        }
    }
}

/// **The caret grows with the panel.**
///
/// A widget builds its own parts, so an inline declaration cannot reach the
/// caret block: the panel writes its width into a one-rule stylesheet of its
/// own at the scale it chose ([`scale_sheet`]). Without that the caret is
/// `default.css`'s hairline at every scale, in a field whose glyphs are four
/// times the size.
#[test]
fn the_caret_is_as_wide_as_the_style_says_at_every_scale() {
    let atlas = atlas();
    let extent = (960, 720);
    let mut widths = Vec::new();
    for scale in [1, 4] {
        let style = ConsoleStyle::pixel_art(scale);
        let mut content = panel(&[], "help");
        // Two frames: a replaced sheet takes effect when the next frame begins.
        content.layout_with(extent, &atlas, &style, idle(), quiet());
        let layout = content.layout_with(extent, &atlas, &style, idle(), quiet());
        let mut dl = DrawList::new();
        content.render(&mut dl, &layout, &atlas);
        let caret = dl
            .commands()
            .iter()
            .find_map(|command| match command {
                DrawCommand::Rect { min, max, color }
                    if *color == CARET_COLOUR && min.x >= layout.text_pos().x - 1e-3 =>
                {
                    Some(max.x - min.x)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("scale {scale} drew no caret"));
        assert!(
            (caret - style.caret_width).abs() < 1e-3,
            "scale {scale} drew a {caret}-pixel caret, not {}",
            style.caret_width,
        );
        widths.push(caret);
    }
    assert!(
        widths[1] > widths[0],
        "the caret is {} pixels wide at every scale",
        widths[0],
    );
}

/// **A bigger window gets a bigger console**, and every window gets one that
/// shows the log rows and the input columns the floors ask for.
#[test]
fn a_bigger_window_gets_a_bigger_console() {
    let atlas = atlas();
    let mut content = panel(&["one"], "");
    let small = still(&mut content, (640, 480)).style().scale;
    let large = still(&mut content, (3840, 2160)).style().scale;
    assert!(
        large > small,
        "640x480 chose {small} and 3840x2160 chose {large}",
    );
    assert!(large <= MenuStyle::MAX_SCALE as f32);

    for extent in EXTENTS {
        let layout = still(&mut content, extent);
        assert!(
            layout.log_rows() >= MINIMUM_LOG_ROWS,
            "{extent:?}: {} log rows at scale {}",
            layout.log_rows(),
            layout.style().scale,
        );
        assert!(
            layout.field_columns(&atlas) >= MINIMUM_FIELD_COLUMNS,
            "{extent:?}: {} input columns at scale {}",
            layout.field_columns(&atlas),
            layout.style().scale,
        );
    }
}

// ---------------------------------------------------------------------------
// Where the panel's parts land
// ---------------------------------------------------------------------------

/// **The panel is the top slice of the frame and everything is inside it**,
/// at every aspect ratio: the log above the input row, the input row above
/// the panel's bottom edge, and the button at the right-hand end of it.
#[test]
fn the_panel_is_the_top_of_the_frame_and_holds_its_parts() {
    let mut content = panel(&["one", "two"], "help");
    for extent in EXTENTS {
        let layout = still(&mut content, extent);
        let (min, max) = layout.panel();
        assert_eq!(min, Vec2::ZERO, "{extent:?}: the panel left the top-left");
        assert_eq!(
            max.x, extent.0 as f32,
            "{extent:?}: the panel is not full width"
        );
        assert!(
            (max.y - (extent.1 as f32 * CONSOLE_HEIGHT_FRACTION).round()).abs() < 1e-3,
            "{extent:?}: the panel is {} tall, not {CONSOLE_HEIGHT_FRACTION} of the frame",
            max.y,
        );

        let (log_min, log_max) = layout.log();
        let (field_min, field_max) = layout.field();
        let (send_min, send_max) = layout.send();
        for (name, (part_min, part_max)) in [
            ("the log", (log_min, log_max)),
            ("the field", (field_min, field_max)),
            ("the button", (send_min, send_max)),
        ] {
            assert!(
                part_min.x >= min.x
                    && part_min.y >= min.y
                    && part_max.x <= max.x
                    && part_max.y <= max.y,
                "{extent:?}: {name} at {part_min:?}..{part_max:?} escapes the panel",
            );
            assert!(
                part_max.x >= part_min.x && part_max.y >= part_min.y,
                "{extent:?}: {name} is inside out",
            );
        }
        assert!(
            log_max.y <= field_min.y,
            "{extent:?}: the log runs into the input row",
        );
        assert!(
            field_max.x <= send_min.x,
            "{extent:?}: the field runs into the Send button",
        );
        assert!(
            send_max.x >= max.x - layout.style().padding.x * 2.0,
            "{extent:?}: the Send button is not at the right-hand edge",
        );
    }
}

/// **The prompt, the typed line and the button are one row**, and the line
/// starts after the prompt rather than under it.
#[test]
fn the_prompt_the_line_and_the_button_share_the_input_row() {
    let atlas = atlas();
    let mut content = panel(&[], "antialiasing");
    let layout = still(&mut content, (960, 720));
    let style = layout.style();

    let prompt_width = atlas.text_width(PROMPT, style.text_size / NATURAL_FONT_SIZE);
    assert!(
        (layout.text_pos().x - layout.prompt_pos().x - prompt_width).abs() < 1e-3,
        "the typed line does not start one prompt past the prompt",
    );
    assert_eq!(layout.text_pos().y, layout.prompt_pos().y);

    let (field_min, field_max) = layout.field();
    assert!(
        layout.prompt_pos().y >= field_min.y
            && layout.prompt_pos().y + style.row_height() <= field_max.y + 1e-3,
        "the prompt is not inside the input row",
    );
    let (send_min, send_max) = layout.send();
    assert!(
        send_min.y >= field_min.y - 1e-3 && send_max.y <= field_max.y + 1e-3,
        "the button {send_min:?}..{send_max:?} is not on the input row \
         {field_min:?}..{field_max:?}",
    );
}

/// **A frame draws the log, the prompt, the typed line and the button** —
/// in that order, and with a scrim behind them.
#[test]
fn a_frame_draws_the_log_the_prompt_the_line_and_the_button() {
    let mut content = panel(&["first", "second"], "help fps");
    let (layout, dl) = drawn(&mut content, (960, 720));

    assert_eq!(
        texts(&dl),
        ["first", "second", PROMPT, "help fps", SEND_LABEL],
    );
    assert!(
        matches!(
            dl.commands().first(),
            Some(DrawCommand::Rect { min, max, color })
                if *min == layout.panel().0
                    && *max == layout.panel().1
                    && *color == layout.style().panel_color
        ),
        "the scrim is not the first thing drawn: {:?}",
        dl.commands().first(),
    );
}

/// **Every log line is drawn above the input row, and the newest is at the
/// bottom of the box.**
///
/// Without the first half the log could be laid out over the field and every
/// other test here would pass — the text would still be in the panel. The
/// second half is what `justify-content: flex-end` and the log box's clip buy:
/// a log longer than the box drops its *oldest* rows off the top.
#[test]
fn no_log_line_is_drawn_over_the_input_row() {
    let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut content = panel(&borrowed, "");
    for extent in EXTENTS {
        let (layout, dl) = drawn(&mut content, extent);
        let style = layout.style();
        let rows: Vec<(Vec2, String, ClipRect)> = dl
            .commands()
            .iter()
            .zip(dl.clips())
            .filter_map(|(command, clip)| match command {
                DrawCommand::Text { pos, text, .. } if text.starts_with("line ") => {
                    Some((*pos, text.clone(), *clip))
                }
                _ => None,
            })
            .collect();
        assert!(!rows.is_empty(), "{extent:?}: the panel drew no log at all");

        // **The log box clips**, so a row it has no room for is cut rather than
        // culled: what has to hold is that every row is drawn under the box's
        // own clip, and that the clip stops above the input row.
        for (pos, text, clip) in &rows {
            assert!(
                clip.min.y >= layout.log().0.y - 1e-3 && clip.max.y <= layout.log().1.y + 1e-3,
                "{extent:?}: {text:?} at {pos:?} is clipped to {:?}..{:?}, which is \
                 not the log box {:?}..{:?}",
                clip.min,
                clip.max,
                layout.log().0,
                layout.log().1,
            );
            assert!(
                clip.max.y <= layout.field().0.y + 1e-3,
                "{extent:?}: {text:?} is drawn under a clip that reaches the input row",
            );
        }

        // And what the box shows is the newest lines, the newest of them on its
        // bottom edge.
        let shown: Vec<&(Vec2, String, ClipRect)> = rows
            .iter()
            .filter(|(pos, _, _)| pos.y >= layout.log().0.y - 1e-3)
            .collect();
        assert_eq!(
            shown.len(),
            layout.log_rows(),
            "{extent:?}: {} rows land inside a box that shows {}",
            shown.len(),
            layout.log_rows(),
        );
        let (bottom, newest, _) = shown.last().expect("a row inside the box");
        assert_eq!(
            newest, "line 39",
            "{extent:?}: the newest line is not the last one inside the box",
        );
        assert!(
            (bottom.y + style.row_height() - layout.log().1.y).abs() < 1e-3,
            "{extent:?}: the newest line sits at {bottom:?}, not on the log box's \
             bottom edge {}",
            layout.log().1.y,
        );
    }
}

/// **A typed line longer than the input box is clipped to it**, text and caret
/// both.
///
/// The tree clips now, where the field used to window itself: the input is an
/// `overflow: hidden` block that scrolls its line under its caret, so what
/// proves this is the **clip rectangle** the emission pushes — a glyph run may
/// well start left of the box and be cut by it, which a test on the run's own
/// extent would call a failure.
#[test]
fn a_long_line_stays_inside_the_input_box() {
    let atlas = atlas();
    let extent = (960, 720);
    let long = "log warn,crcbl_vk=trace,crcbl_render=debug,crcbl_scene=trace,crcbl_ui=trace";
    let mut content = panel(&[], long);
    let (layout, dl) = drawn(&mut content, extent);
    assert!(
        long.chars().count() > layout.field_columns(&atlas),
        "the line is not longer than the box, so this proves nothing",
    );

    let right_edge = layout.field().1.x;
    let mut drew_some_of_it = false;
    for (command, clip) in dl.commands().iter().zip(dl.clips()) {
        let DrawCommand::Text { text, .. } = command else {
            continue;
        };
        if !long.contains(text.as_str()) {
            continue;
        }
        drew_some_of_it = true;
        assert!(
            clip.min.x >= layout.text_pos().x - 1e-3 && clip.max.x <= right_edge + 1e-3,
            "the typed line is drawn under a clip {:?}..{:?} wider than the input \
             box {}..{right_edge}",
            clip.min,
            clip.max,
            layout.text_pos().x,
        );
        assert!(
            clip.max.x - clip.min.x
                < atlas.text_width(long, layout.style().text_size / NATURAL_FONT_SIZE),
            "the clip is wider than the whole line, so it clips nothing",
        );
    }
    assert!(drew_some_of_it, "none of the typed line was drawn");
}

/// A framebuffer too small for the console lays out without inverting a
/// rectangle and draws no text over the frame.
#[test]
fn a_frame_with_no_room_lays_out_without_inverting_anything() {
    for extent in [(0, 0), (1, 1), (32, 24)] {
        let mut content = panel(&["one", "two"], "help");
        let (layout, dl) = drawn(&mut content, extent);
        for (name, (min, max)) in [
            ("the panel", layout.panel()),
            ("the log", layout.log()),
            ("the field", layout.field()),
        ] {
            assert!(
                max.x >= min.x && max.y >= min.y,
                "{extent:?}: {name} is inside out at {min:?}..{max:?}",
            );
        }
        assert_eq!(layout.log_rows(), 0, "{extent:?}: a log row fitted");
        for text in texts(&dl) {
            assert_ne!(text, "one", "{extent:?}: a log line was drawn anyway");
        }
    }
}

// ---------------------------------------------------------------------------
// Submitting
// ---------------------------------------------------------------------------

/// **`Enter` and the Send button submit the same line**, and both leave the
/// field empty — the decision-6 requirement that the button is not a second
/// path with its own behaviour.
#[test]
fn enter_and_the_send_button_submit_the_same_line() {
    let extent = (960, 720);

    let mut typed = panel(&[], "antialiasing cmaa2");
    assert_eq!(typed.submit().as_deref(), Some("antialiasing cmaa2"));
    assert!(typed.line().is_empty(), "the field kept the sent line");
    assert_eq!(typed.submit(), None, "an empty field submitted a command");

    let mut clicked = panel(&[], "antialiasing cmaa2");
    let mut ui = UiState::new();
    let layout = still(&mut clicked, extent);
    let on_button = (layout.send().0 + layout.send().1) * 0.5;

    let layout = frame(&mut clicked, extent, press_at(on_button), quiet());
    assert_eq!(
        clicked.point(&layout, &mut ui, press_at(on_button)),
        ConsoleInput::Nothing,
    );
    assert_eq!(
        clicked.send_state(),
        ButtonState::Pressed,
        "the press did not reach the button's art",
    );

    let layout = frame(&mut clicked, extent, release_at(on_button), quiet());
    assert_eq!(
        clicked.point(&layout, &mut ui, release_at(on_button)),
        ConsoleInput::Submitted("antialiasing cmaa2".to_owned()),
    );
    assert!(clicked.line().is_empty());
}

/// **Submitting a line puts the log back at its bottom**, whichever way it
/// was sent — the answer lands there, and a reader who had scrolled back
/// would otherwise be looking at old lines while it arrives.
#[test]
fn a_submitted_line_returns_the_log_to_its_newest_lines() {
    let lines: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut panel = panel(&lines, "help");
    panel.log_mut().scroll_by(10);
    assert_eq!(panel.log().scroll(), 10, "the view did not scroll back");

    assert_eq!(panel.submit().as_deref(), Some("help"));
    assert_eq!(
        panel.log().scroll(),
        0,
        "the log stayed scrolled back after a line was sent",
    );

    // And a blank submission is still a submission for this purpose: the
    // person pressed Enter to get back to the bottom, which a terminal also
    // does.
    panel.log_mut().scroll_by(10);
    assert_eq!(panel.submit(), None);
    assert_eq!(panel.log().scroll(), 0);
}

/// **A press that starts on the button and is released off it sends
/// nothing**, and a press that never touches it sends nothing either.
#[test]
fn a_press_that_leaves_the_button_sends_nothing() {
    let extent = (960, 720);
    let mut content = panel(&[], "quit");
    let mut ui = UiState::new();
    let layout = still(&mut content, extent);
    let on_button = (layout.send().0 + layout.send().1) * 0.5;
    let elsewhere = layout.log().0 + Vec2::splat(1.0);

    let layout = frame(&mut content, extent, press_at(on_button), quiet());
    content.point(&layout, &mut ui, press_at(on_button));
    let layout = frame(&mut content, extent, release_at(elsewhere), quiet());
    assert_eq!(
        content.point(&layout, &mut ui, release_at(elsewhere)),
        ConsoleInput::Nothing,
    );
    assert_eq!(content.line(), "quit", "the line was sent anyway");

    let layout = frame(&mut content, extent, press_at(elsewhere), quiet());
    content.point(&layout, &mut ui, press_at(elsewhere));
    let layout = frame(&mut content, extent, release_at(elsewhere), quiet());
    assert_eq!(
        content.point(&layout, &mut ui, release_at(elsewhere)),
        ConsoleInput::Nothing,
    );
    assert_eq!(content.line(), "quit");
}

// ---------------------------------------------------------------------------
// The completion list
// ---------------------------------------------------------------------------

/// **The completion rows hang under the field with the matched head
/// highlighted**, capped at [`COMPLETION_ROWS`], and lined up with the
/// typed token rather than with the panel's edge.
///
/// The line-up is what holds [`text_inset`] — the one piece of the input row's
/// box the build needs *before* the tree lays it out — to the tree's own
/// answer, which is what [`ConsoleLayout::text_pos`] is read off.
#[test]
fn the_completion_rows_highlight_the_matched_head() {
    let atlas = atlas();
    let mut content = panel(&[], "r_a");
    content.set_completion("r_a", &["r_ao_view", "r_ambient"]);
    let (layout, dl) = drawn(&mut content, (960, 720));
    let style = *layout.style();
    assert_eq!(layout.completion().len(), 2);

    let rows = placed(&dl);
    // The prompt, the typed line and the button come first; what follows is
    // two candidates, each split into a matched head and a tail.
    let tail = &rows[rows.len() - 4..];
    assert_eq!(
        tail.iter()
            .map(|(_, text, color)| (text.as_str(), *color))
            .collect::<Vec<_>>(),
        [
            ("r_a", style.match_color),
            ("o_view", style.candidate_color),
            ("r_a", style.match_color),
            ("mbient", style.candidate_color),
        ],
    );
    assert!(
        (tail[1].0.x - tail[0].0.x - atlas.text_width("r_a", style.text_size / NATURAL_FONT_SIZE))
            .abs()
            < 1e-3,
        "the tail is not drawn one matched head to the right of it",
    );
    assert_eq!(
        tail[0].0.x,
        layout.text_pos().x,
        "the candidates do not line up with the typed token",
    );
    assert!(
        tail[2].0.y - tail[0].0.y >= style.row_height() - 1e-3,
        "the two candidates are on the same row",
    );
    for row in layout.completion() {
        assert!(
            row.0.y >= layout.panel().1.y - 1e-3,
            "a candidate row is drawn inside the panel, over the input",
        );
        assert!(
            row.1.y <= layout.screen().y + 1e-3,
            "a candidate row hangs off the bottom of the frame",
        );
    }
}

/// The list is capped, and it is capped by the frame as well as by
/// [`COMPLETION_ROWS`] — a row that would hang off the bottom is not built,
/// because nothing clips it.
#[test]
fn the_completion_list_is_capped_by_the_rows_and_by_the_frame() {
    let names: Vec<String> = (0..COMPLETION_ROWS + 4)
        .map(|i| format!("cmd_{i}"))
        .collect();
    let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut content = panel(&[], "cmd_");
    content.set_completion("cmd_", &borrowed);

    let roomy = still(&mut content, (960, 720));
    assert_eq!(roomy.completion().len(), COMPLETION_ROWS);

    // A frame with almost nothing below the panel fits fewer rows than the
    // cap, and every one of them still ends inside the frame.
    let cramped = still(&mut content, (960, 80));
    assert!(
        cramped.completion().len() < COMPLETION_ROWS,
        "an 80-pixel frame offered {} rows below the panel",
        cramped.completion().len(),
    );
    for row in cramped.completion() {
        assert!(row.1.y <= cramped.screen().y + 1e-3);
    }

    content.clear_completion();
    assert!(
        still(&mut content, (960, 720)).completion().is_empty(),
        "the candidates outlived the completion",
    );
}

// ---------------------------------------------------------------------------
// The on-screen keyboard, which keeps its own layout
// ---------------------------------------------------------------------------

/// **A hidden keyboard is not a keyboard drawn off screen**: it lays out no
/// keys, claims none of the frame, and takes no press.
///
/// The half that matters is [`ConsoleLayout::covers`]. The console hands
/// that answer to the loop as "the press was mine", so a hidden keyboard
/// that still claimed its strip would take every tap along the bottom third
/// of the frame away from the game — on every desktop, where the keyboard
/// is never shown at all.
#[test]
fn a_hidden_keyboard_lays_out_nothing_and_claims_nothing() {
    for extent in EXTENTS {
        let mut content = panel(&[], "");
        assert!(!content.keyboard_shown(), "a new panel showed a keyboard");
        let layout = still(&mut content, extent);
        assert!(
            layout.keyboard().keys().is_empty(),
            "a hidden keyboard laid out {} keys",
            layout.keyboard().keys().len(),
        );

        // Where the keys would be if it were showing.
        let mut shown = ConsolePanel::new();
        shown.show_keyboard(true);
        let visible = still(&mut shown, extent);
        let at = key_centre(&visible, KeyCap::Type('q'));

        assert!(
            !layout.covers(at),
            "a hidden keyboard claimed {at} on a {extent:?} frame",
        );
        let mut ui = UiState::new();
        let layout = frame(&mut content, extent, press_at(at), quiet());
        content.point(&layout, &mut ui, press_at(at));
        let layout = frame(&mut content, extent, release_at(at), quiet());
        assert_eq!(
            content.point(&layout, &mut ui, release_at(at)),
            ConsoleInput::Nothing,
            "a hidden keyboard took a press",
        );
        assert!(content.line().is_empty());
    }
}

/// **A tap on a key is reported as that key**, and the strip it was tapped
/// in is the console's so the game never sees the press.
#[test]
fn a_tap_on_the_keyboard_is_the_consoles_and_names_its_key() {
    for extent in EXTENTS {
        let mut content = panel(&[], "");
        content.show_keyboard(true);
        let layout = still(&mut content, extent);
        let at = key_centre(&layout, KeyCap::Type('q'));
        assert!(
            layout.covers(at),
            "the console did not claim its own key at {at} on {extent:?}",
        );

        let mut ui = UiState::new();
        let layout = frame(&mut content, extent, press_at(at), quiet());
        content.point(&layout, &mut ui, press_at(at));
        let layout = frame(&mut content, extent, release_at(at), quiet());
        assert_eq!(
            content.point(&layout, &mut ui, release_at(at)),
            ConsoleInput::Key(KeyCap::Type('q')),
            "a tap on q on a {extent:?} frame reported nothing",
        );
    }
}

/// The keyboard is drawn only when it is showing, and it is drawn **last**
/// so the candidate rows cannot cover the keys they hang over.
#[test]
fn the_keyboard_is_drawn_only_when_it_is_showing() {
    let extent = (600, 900);
    let mut content = panel(&["one"], "q");
    let (_, hidden) = drawn(&mut content, extent);

    content.show_keyboard(true);
    let (_, shown) = drawn(&mut content, extent);

    assert!(
        shown.commands().len() > hidden.commands().len(),
        "showing the keyboard drew nothing: {} commands either way",
        hidden.commands().len(),
    );
    let labels = texts(&shown);
    assert!(
        labels.iter().any(|text| text == "SPACE"),
        "the space bar was not drawn: {labels:?}",
    );
}
