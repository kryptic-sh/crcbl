//! The text input: the engaged rule, the pointer, the clipboard's requests
//! and answers, the parts it draws and where they sit, and the caret's blink
//! and scrolling.

use std::time::Duration;

use super::*;
use crate::console::CARET_BLINK;
use crate::edit::{ClipboardOp, Edit, Motion};
use crate::font::Font;
use crate::font::layout::TextLayout;
use crate::style::PseudoClasses;
use crate::tree::{
    ClipboardAnswer, ClipboardReply, ClipboardRequest, Content, DOUBLE_CLICK_TIME, Engagement,
    Response, TextInputOptions,
};
use crate::widget::NATURAL_FONT_SIZE;

/// Every frame's length unless a test says otherwise.
const FRAME: Duration = Duration::from_millis(16);

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

const fn step(motion: Motion, select: bool) -> Edit {
    Edit::Move { motion, select }
}

/// One frame of a column holding `#name`, a plain text input, then `#pass`, a
/// masked one with a placeholder.
struct Page {
    name: Response,
    pass: Response,
}

fn page(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    text: TextInput,
    values: &mut [String; 2],
) -> Page {
    frame_with_text(ui, pointer, nav, text, |ui| {
        let [name, pass] = values;
        Page {
            name: ui.text_input("#name", name),
            pass: ui.text_input_with(
                "#pass",
                pass,
                TextInputOptions {
                    placeholder: "password",
                    masked: true,
                },
            ),
        }
    })
}

/// The child of `key` whose selector names `class`, as this frame built it.
fn part(ui: &Ui, key: NodeKey, class: &str) -> Option<NodeKey> {
    let parent = ui.nodes.iter().position(|node| node.key == key)?;
    ui.nodes
        .iter()
        .filter(|node| node.parent == Some(parent))
        .find(|node| ui.selectors[node.selector.0..node.selector.1].contains(class))
        .map(|node| node.key)
}

/// The text a span drew this frame.
fn shown(ui: &Ui, span: NodeKey) -> &str {
    let node = ui
        .nodes
        .iter()
        .find(|node| node.key == span)
        .expect("built");
    match node.content {
        Content::Text { start, end } => &ui.text[start..end],
        _ => panic!("not a text span"),
    }
}

/// How wide `text` draws in `span`'s bitmap font, measured by the atlas and
/// nothing of the widget's.
fn bitmap_width(ui: &Ui, span: NodeKey, text: &str) -> f32 {
    let scale = style_of(ui, span).font_size / NATURAL_FONT_SIZE;
    FontAtlas::built_in().text_width(text, scale)
}

/// Clicks `pos`: a press frame and a release frame.
fn click(ui: &mut Ui, pos: Vec2, values: &mut [String; 2]) -> Page {
    page(ui, press(pos), NavInput::default(), quiet(), values);
    page(ui, release(pos), NavInput::default(), quiet(), values)
}

/// A fresh page with `name` in `#name`, laid out once and `#name` engaged by
/// accept.
fn engaged_on(name: &str) -> (Ui, [String; 2]) {
    let mut ui = Ui::new();
    let mut values = [name.to_owned(), String::new()];
    page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    page(&mut ui, idle(), NavInput::NAVIGATION, quiet(), &mut values);
    let engaged = page(&mut ui, idle(), NavInput::ACCEPT, quiet(), &mut values);
    assert_eq!(engaged.name.engagement, Engagement::Began);
    (ui, values)
}

/// **The LOCKED rule on a text input**: focus moves past it, and nothing
/// typed reaches it while it is only focused; accept engages it without
/// taking the text the accepting press committed; while engaged it takes the
/// edits in order, and a navigation step moves neither focus nor the caret;
/// back cancels to the text it engaged with; accept commits.
#[test]
fn a_text_input_engages_takes_edits_cancels_to_its_snapshot_and_commits() {
    let mut ui = Ui::new();
    let mut values = [String::from("hi"), String::new()];
    page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    let landed = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([insert("x")]),
        &mut values,
    );
    assert!(landed.name.focused);
    let passed = page(&mut ui, idle(), DOWN, typed([insert("y")]), &mut values);
    assert!(
        passed.pass.focused,
        "down did not move focus past the input"
    );
    page(&mut ui, idle(), UP, quiet(), &mut values);
    assert_eq!(values, ["hi", ""], "a focused input took typing");

    let began = page(
        &mut ui,
        idle(),
        NavInput::ACCEPT,
        typed([insert(" ")]),
        &mut values,
    );
    assert_eq!(began.name.engagement, Engagement::Began);
    assert_eq!(values[0], "hi", "the accepting press's own text was typed");
    assert!(
        ui.text_editing(),
        "an engaged input is not reported as editing"
    );

    for nav in [RIGHT, LEFT, DOWN, NavInput::NEXT] {
        let held = page(&mut ui, idle(), nav, quiet(), &mut values);
        assert!(
            held.name.focused,
            "{nav:?} moved focus off the engaged input"
        );
        assert_eq!(
            ui.text_caret(held.name.key),
            Some((2, 2)),
            "{nav:?} moved the caret"
        );
    }
    let edited = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([insert("!"), step(Motion::Left, false), insert("?")]),
        &mut values,
    );
    assert_eq!(values[0], "hi?!", "the edits were not taken in order");
    assert!(edited.name.changed);

    let cancelled = page(&mut ui, idle(), NavInput::BACK, quiet(), &mut values);
    assert_eq!(cancelled.name.engagement, Engagement::Cancelled);
    assert_eq!(values[0], "hi", "back did not restore the snapshot");
    assert!(cancelled.name.changed, "the restore was not reported");
    assert!(!ui.text_editing());

    page(&mut ui, idle(), NavInput::ACCEPT, quiet(), &mut values);
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([insert("s")]),
        &mut values,
    );
    let committed = page(&mut ui, idle(), NavInput::ACCEPT, quiet(), &mut values);
    assert_eq!(committed.name.engagement, Engagement::Committed);
    page(&mut ui, idle(), NavInput::BACK, quiet(), &mut values);
    assert!(
        ui.back_requested(),
        "back after the commit was not the caller's"
    );
    assert_eq!(values[0], "his", "commit lost the edit, or back undid it");
}

/// **The pointer places the caret on the nearest boundary, a drag selects,
/// and a double-click selects the word under it** — and a drag that ends on
/// an input that was not engaged leaves it engaged, where a slider's drag
/// does not.
#[test]
fn a_click_places_the_caret_a_drag_selects_and_a_double_click_selects_a_word() {
    let mut ui = Ui::new();
    let mut values = [String::from("hello world"), String::new()];
    let first = page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    let key = first.name.key;
    let span = part(&ui, key, ".text-input-text").expect("the text is a part");
    let (origin, bottom) = rect(&ui, span);
    let y = (origin.y + bottom.y) * 0.5;
    let advance = bitmap_width(&ui, span, "M");
    let at = |stops: f32| Vec2::new(origin.x + stops * advance, y);

    let clicked = click(&mut ui, at(3.4), &mut values);
    assert_eq!(clicked.name.engagement, Engagement::Began);
    assert_eq!(
        ui.text_caret(key),
        Some((3, 3)),
        "3.4 glyphs in is not stop 3"
    );
    // Two presses further apart in time than a double-click are two clicks.
    let slow = TextInput {
        dt: DOUBLE_CLICK_TIME,
        ..quiet()
    };
    let slow_click = |ui: &mut Ui, pos, values: &mut [String; 2]| {
        page(ui, press(pos), NavInput::default(), slow.clone(), values);
        page(ui, release(pos), NavInput::default(), slow.clone(), values);
    };
    slow_click(&mut ui, at(3.6), &mut values);
    assert_eq!(
        ui.text_caret(key),
        Some((4, 4)),
        "3.6 glyphs in is not stop 4"
    );

    // Far enough from the last press that it is no double-click.
    page(
        &mut ui,
        press(at(1.2)),
        NavInput::default(),
        quiet(),
        &mut values,
    );
    page(
        &mut ui,
        press(at(7.6)),
        NavInput::default(),
        quiet(),
        &mut values,
    );
    page(
        &mut ui,
        release(at(7.6)),
        NavInput::default(),
        quiet(),
        &mut values,
    );
    assert_eq!(
        ui.text_caret(key),
        Some((8, 1)),
        "the drag did not select 1..8"
    );
    assert_eq!(ui.engaged(), Some(key));

    click(&mut ui, at(8.5), &mut values);
    click(&mut ui, at(8.5), &mut values);
    assert_eq!(
        ui.text_caret(key),
        Some((11, 6)),
        "a double-click on `world` did not select it"
    );

    slow_click(&mut ui, at(2.2), &mut values);
    slow_click(&mut ui, at(2.2), &mut values);
    assert_eq!(
        ui.text_caret(key),
        Some((2, 2)),
        "slow clicks selected a word"
    );
    assert_eq!(values[0], "hello world", "the pointer changed the text");

    let mut fresh = Ui::new();
    let mut values = [String::from("hello world"), String::new()];
    page(
        &mut fresh,
        idle(),
        NavInput::default(),
        quiet(),
        &mut values,
    );
    page(
        &mut fresh,
        press(at(1.0)),
        NavInput::default(),
        quiet(),
        &mut values,
    );
    page(
        &mut fresh,
        press(at(5.0)),
        NavInput::default(),
        quiet(),
        &mut values,
    );
    page(
        &mut fresh,
        release(at(5.0)),
        NavInput::default(),
        quiet(),
        &mut values,
    );
    assert_eq!(
        fresh.engaged(),
        Some(key),
        "a drag-select did not end engaged"
    );
    assert_eq!(fresh.text_caret(key), Some((5, 1)));
}

/// **The selection highlight spans exactly the selected glyphs, and the caret
/// sits after the last typed glyph**, both measured against the atlas's own
/// advances — in the bitmap font, and in a parsed font against where
/// [`TextLayout`] draws each glyph, kerning included.
#[test]
fn the_selection_and_the_caret_sit_on_the_glyphs_they_name() {
    let (mut ui, mut values) = engaged_on("abcdef");
    let edits = [
        step(Motion::Home, false),
        step(Motion::Right, false),
        step(Motion::Right, true),
        step(Motion::Right, true),
    ];
    let selected = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed(edits),
        &mut values,
    );
    let key = selected.name.key;
    let span = part(&ui, key, ".text-input-text").expect("text");
    let highlight = part(&ui, key, ".text-input-selection").expect("a selection is drawn");
    let (text_min, text_max) = rect(&ui, span);
    let (min, max) = rect(&ui, highlight);
    assert_eq!(
        min.x,
        text_min.x + bitmap_width(&ui, span, "a"),
        "left edge"
    );
    assert_eq!(
        max.x,
        text_min.x + bitmap_width(&ui, span, "abc"),
        "right edge"
    );
    assert_eq!(
        (min.y, max.y),
        (text_min.y, text_max.y),
        "not the line's height"
    );

    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([step(Motion::End, false), insert("g")]),
        &mut values,
    );
    assert!(
        part(&ui, key, ".text-input-selection").is_none(),
        "typing left a selection"
    );
    let caret = part(&ui, key, ".text-input-caret").expect("an engaged input has a caret");
    let (caret_min, caret_max) = rect(&ui, caret);
    let (text_min, text_max) = rect(&ui, span);
    assert_eq!(caret_min.x, text_min.x + bitmap_width(&ui, span, "abcdefg"));
    assert_eq!(
        caret_max.x - caret_min.x,
        1.0,
        "not default.css's caret width"
    );
    assert_eq!((caret_min.y, caret_max.y), (text_min.y, text_max.y));

    // A parsed font: the boundaries are glyph positions with kerning.
    let mut ui = Ui::new();
    ui.add_stylesheet(
        "sans.css",
        "text-input { font-family: sans-serif; font-size: 20px; }",
    );
    let mut values = [String::from("AVAWATAY"), String::new()];
    page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    page(&mut ui, idle(), NavInput::NAVIGATION, quiet(), &mut values);
    page(&mut ui, idle(), NavInput::ACCEPT, quiet(), &mut values);
    let edits = [
        step(Motion::Home, false),
        step(Motion::Right, false),
        step(Motion::Right, false),
        step(Motion::Right, true),
        step(Motion::Right, true),
        step(Motion::Right, true),
    ];
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed(edits),
        &mut values,
    );
    let span = part(&ui, key, ".text-input-text").expect("text");
    let style = style_of(&ui, span);
    let font = Font::sans();
    let layout = TextLayout::new(
        font,
        &values[0],
        style.font_size,
        style.text_line_height(font),
        None,
    );
    let glyph_x = |index: usize| layout.glyphs()[index].offset.x;
    let advance = font.advance(font.glyph_id('A')) * font.metrics().scale(style.font_size);
    assert!(
        (glyph_x(1) - advance).abs() > 0.5,
        "`AV` is not kerned, so the claim cannot tell kerning from advances"
    );
    let (text_min, _) = rect(&ui, span);
    let (min, max) = rect(
        &ui,
        part(&ui, key, ".text-input-selection").expect("selected"),
    );
    assert_eq!(
        min.x,
        (text_min.x + glyph_x(2)).round(),
        "left edge in the parsed font"
    );
    assert_eq!(
        max.x,
        (text_min.x + glyph_x(5)).round(),
        "right edge in the parsed font"
    );
}

/// **A line longer than the input scrolls to keep its caret in view**: the
/// caret inside the content box and the end of the line against it; Home
/// scrolls back to the start; and an input no longer engaged shows its start.
#[test]
fn a_long_line_scrolls_to_keep_its_caret_in_view() {
    let (mut ui, mut values) = engaged_on("");
    let line = "abcdefghijklmnopqrstuvwxyz0123456789";
    let typed_line = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([insert(line)]),
        &mut values,
    );
    let key = typed_line.name.key;
    let span = part(&ui, key, ".text-input-text").expect("text");
    let caret = part(&ui, key, ".text-input-caret").expect("caret");
    let view = ui.store.by_key(key).expect("stored").content_box();
    let (text_min, _) = rect(&ui, span);
    let (caret_min, caret_max) = rect(&ui, caret);
    assert!(
        bitmap_width(&ui, span, line) > view.1.x - view.0.x,
        "the line fits, so nothing scrolls"
    );
    assert!(ui.scroll_offset_of(key).x > 0.0, "the input did not scroll");
    assert_eq!(
        caret_max.x, view.1.x,
        "the caret is not against the right edge"
    );
    assert_eq!(
        caret_min.x,
        text_min.x + bitmap_width(&ui, span, line),
        "the caret is not after the last glyph"
    );

    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([step(Motion::Home, false)]),
        &mut values,
    );
    let (caret_min, _) = rect(&ui, part(&ui, key, ".text-input-caret").expect("caret"));
    assert_eq!(ui.scroll_offset_of(key).x, 0.0, "Home did not scroll back");
    assert_eq!(
        caret_min.x, view.0.x,
        "the caret is not at the content's left"
    );

    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([step(Motion::End, false)]),
        &mut values,
    );
    assert!(
        ui.scroll_offset_of(key).x > 0.0,
        "End did not scroll to the end"
    );
    page(&mut ui, idle(), NavInput::ACCEPT, quiet(), &mut values);
    assert_eq!(
        ui.scroll_offset_of(key).x,
        0.0,
        "a committed input still shows the end of its line"
    );
}

/// **Copy and cut offer the selection, paste asks for a read, and a later
/// answer replaces the selection** — naming the input that asked, so an
/// answer to another input, or to one no longer engaged, changes nothing. A
/// refusal sets `:refused`, which draws `default.css`'s red border until the
/// next edit, and a masked input offers nothing.
#[test]
fn the_clipboard_is_requests_out_and_answers_back() {
    let (mut ui, mut values) = engaged_on("copy me");
    let copied = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([Edit::SelectAll, Edit::Copy]),
        &mut values,
    );
    let key = copied.name.key;
    assert_eq!(
        ui.take_clipboard_requests(),
        [ClipboardRequest {
            from: key,
            op: ClipboardOp::Offer("copy me".to_owned()),
        }]
    );
    assert!(
        ui.take_clipboard_requests().is_empty(),
        "taking did not drain"
    );

    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([
            step(Motion::Home, false),
            step(Motion::WordRight, true),
            Edit::Cut,
        ]),
        &mut values,
    );
    assert_eq!(values[0], " me");
    assert_eq!(
        ui.take_clipboard_requests(),
        [ClipboardRequest {
            from: key,
            op: ClipboardOp::Offer("copy".to_owned()),
        }]
    );

    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([step(Motion::End, true), Edit::Paste]),
        &mut values,
    );
    assert_eq!(
        ui.take_clipboard_requests(),
        [ClipboardRequest {
            from: key,
            op: ClipboardOp::Read,
        }]
    );
    let answer = |to, reply| TextInput {
        clipboard: vec![ClipboardAnswer { to, reply }],
        ..quiet()
    };
    let other = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        answer(copied.pass.key, ClipboardReply::Text("wrong".to_owned())),
        &mut values,
    );
    assert_eq!(values, [" me", ""], "an answer to another input landed");
    let pasted = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        answer(key, ClipboardReply::Text("pasted".to_owned())),
        &mut values,
    );
    assert!(pasted.name.changed && !other.name.changed);
    assert_eq!(
        values[0], "pasted",
        "the read did not replace the selection"
    );

    let refused_border = linear("#e5484d");
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        answer(key, ClipboardReply::Refused),
        &mut values,
    );
    let node = ui.nodes.iter().find(|node| node.key == key).expect("built");
    assert!(node.pseudo.contains(PseudoClasses::REFUSED), "no :refused");
    assert_eq!(style_of(&ui, key).border_color, refused_border);
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([insert("!")]),
        &mut values,
    );
    assert_ne!(
        style_of(&ui, key).border_color,
        refused_border,
        "an edit did not clear :refused"
    );

    page(&mut ui, idle(), NavInput::ACCEPT, quiet(), &mut values);
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        answer(key, ClipboardReply::Text("late".to_owned())),
        &mut values,
    );
    assert_eq!(values[0], "pasted!", "a read landed in a committed input");

    // The masked input: engaged, typed into, and asked to copy and cut.
    page(&mut ui, idle(), DOWN, quiet(), &mut values);
    page(&mut ui, idle(), NavInput::ACCEPT, quiet(), &mut values);
    let masked = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        typed([insert("secret"), Edit::SelectAll, Edit::Copy, Edit::Cut]),
        &mut values,
    );
    assert!(
        ui.take_clipboard_requests().is_empty(),
        "a masked input offered"
    );
    assert_eq!(values[1], "secret", "a masked input cut");
    let span = part(&ui, masked.pass.key, ".text-input-text").expect("text");
    assert_eq!(shown(&ui, span), "******", "the value was drawn unmasked");
}

/// **The placeholder shows while the value is empty, in `default.css`'s
/// dimmed colour, and the text replaces it once typed.**
#[test]
fn the_placeholder_shows_while_the_value_is_empty() {
    let mut ui = Ui::new();
    let mut values = [String::new(), String::new()];
    let first = page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    let placeholder = part(&ui, first.pass.key, ".text-input-placeholder").expect("placeholder");
    assert_eq!(shown(&ui, placeholder), "password");
    assert_eq!(style_of(&ui, placeholder).color, linear("#7a8190"));
    assert!(part(&ui, first.pass.key, ".text-input-text").is_none());
    let empty = part(&ui, first.name.key, ".text-input-text").expect("an empty span");
    assert_eq!(
        shown(&ui, empty),
        "",
        "no placeholder asked for, and one shown"
    );
    let (min, max) = rect(&ui, empty);
    assert!(max.y > min.y, "an empty input collapsed to no line");

    values[1] = "ab".to_owned();
    let typed_in = page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    assert!(part(&ui, typed_in.pass.key, ".text-input-placeholder").is_none());
    let text = part(&ui, typed_in.pass.key, ".text-input-text").expect("text");
    assert_eq!(shown(&ui, text), "**");
}

/// **The caret blinks on the frame clock and shows solid from an edit**: it
/// is hidden once [`CARET_BLINK`] has passed with nothing typed, back after
/// another, and shown the frame an edit lands — its background transparent
/// while hidden, so the blink moves no layout.
#[test]
fn the_caret_blinks_on_the_frame_clock() {
    let (mut ui, mut values) = engaged_on("abc");
    let shown_colour = linear("#3d8bfd");
    let caret_colour =
        |ui: &Ui, key| style_of(ui, part(ui, key, ".text-input-caret").expect("caret")).background;
    let tick = |dt| TextInput { dt, ..quiet() };
    let key = page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        tick(CARET_BLINK / 2),
        &mut values,
    )
    .name
    .key;
    assert_eq!(
        caret_colour(&ui, key),
        shown_colour,
        "hidden before the interval"
    );
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        tick(CARET_BLINK / 2),
        &mut values,
    );
    assert_eq!(caret_colour(&ui, key), [0.0; 4], "shown past the interval");
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        tick(CARET_BLINK),
        &mut values,
    );
    assert_eq!(
        caret_colour(&ui, key),
        shown_colour,
        "not back after another"
    );
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        tick(CARET_BLINK),
        &mut values,
    );
    assert_eq!(caret_colour(&ui, key), [0.0; 4]);
    page(
        &mut ui,
        idle(),
        NavInput::NAVIGATION,
        TextInput {
            dt: CARET_BLINK,
            ..typed([step(Motion::Left, false)])
        },
        &mut values,
    );
    assert_eq!(
        caret_colour(&ui, key),
        shown_colour,
        "a caret move left it hidden"
    );
}

/// **A value changed from outside is taken as it stands**, the caret held
/// inside it, and a disabled input takes no edits.
#[test]
fn an_outside_change_and_a_disabled_input() {
    let (mut ui, mut values) = engaged_on("abcdef");
    values[0] = "ab".to_owned();
    let shrunk = page(&mut ui, idle(), NavInput::NAVIGATION, quiet(), &mut values);
    assert_eq!(ui.text_caret(shrunk.name.key), Some((2, 2)));
    assert!(
        !shrunk.name.changed,
        "the caller's own change was reported back"
    );

    let mut values = [String::from("keep"), String::new()];
    let mut ui = Ui::new();
    let build = |ui: &mut Ui, nav, text, values: &mut [String; 2]| {
        frame_with_text(ui, idle(), nav, text, |ui| {
            ui.enabled(false, |ui| {
                ui.text_input("#off", &mut values[0]);
            });
        })
    };
    build(&mut ui, NavInput::default(), quiet(), &mut values);
    build(&mut ui, NavInput::ACCEPT, typed([insert("x")]), &mut values);
    build(
        &mut ui,
        NavInput::NAVIGATION,
        typed([insert("x")]),
        &mut values,
    );
    assert_eq!(values[0], "keep");
    assert_eq!(ui.engaged(), None, "a disabled input engaged");
}

/// **A value set from outside loses its line breaks**, as HTML strips them
/// from a text input's value: the input draws one line, hands the caller the
/// stripped value and reports that change once — not on every later frame.
#[test]
fn an_outside_value_with_a_line_break_draws_on_one_line() {
    let mut ui = Ui::new();
    let mut values = [String::from("ab"), String::new()];
    page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    values[0] = "ab\ncd\r\n".to_owned();
    let stripped = page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    let text = part(&ui, stripped.name.key, ".text-input-text").expect("text");
    assert_eq!(shown(&ui, text), "abcd");
    assert_eq!(values[0], "abcd");
    assert!(stripped.name.changed, "the stripped value went unreported");
    let steady = page(&mut ui, idle(), NavInput::default(), quiet(), &mut values);
    assert!(!steady.name.changed, "a stripped value changed again");
}
