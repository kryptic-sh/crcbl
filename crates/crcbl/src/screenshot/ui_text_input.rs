//! [`Scene::UiTextInput`](super::Scene::UiTextInput)'s content: rung 7's
//! single-line text input — `docs/plan/07-ui-debug.md` — four inputs on one
//! page, styled by the engine's `default.css` and driven by a scripted pad,
//! typing and pointer, so that what the input promises can be read back off
//! the frame.
//!
//! A scene of its own rather than more of `ui_widgets`: that page's left
//! column is full at the 256×192 every UI golden is blessed at, and inputs
//! added to it would move every widget it already measures.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─ .page ──────────────────┐
//!   │ ╔#long═══════════════╗   │  engaged: a line longer than the input,
//!   │ ║…fox jumps over|    ║   │  typed to its end
//!   │ ┌#picked─────────────┐   │  a drag held over it, selecting glyphs
//!   │ │se▓▓▓▓ me           │   │  UI_TEXT_INPUT_PICK
//!   │ ┌#empty──────────────┐   │  no value: the placeholder, dimmed
//!   │ │name                │   │
//!   │ ┌#secret─────────────┐   │  masked: one MASK per character
//!   │ │*******             │   │
//!   └──────────────────────────┘
//! ```
//!
//! The script, one frame each: the pad speaks and focus lands on `#long`;
//! accept engages it; [`UI_TEXT_INPUT_LONG`] is typed into it; a press lands on
//! `#picked` at the start of [`UI_TEXT_INPUT_PICK`] and drags to its end, and
//! the frame is drawn with the press still held. A press does not commit an
//! engaged input — only a click does — so `#long` keeps its caret while
//! `#picked` shows the selection a held press draws.
//!
//! Every colour and length the frame is measured against is a constant here,
//! and the tests below hold each colour constant to what `default.css`
//! resolves to.

use std::time::Duration;

use glam::Vec2;

use crate::ui::draw_list::DrawList;
use crate::ui::edit::Edit;
use crate::ui::style::Declaration;
use crate::ui::text::{FontAtlas, GLYPH_ADVANCE};
use crate::ui::tree::{
    AvailableSpace, LengthAuto, NavInput, NodeKey, TextInput, TextInputOptions, Ui,
};
use crate::ui::widget::PointerInput;

/// The page's fill, as the scene's stylesheet writes it: sRGB bytes.
pub const UI_TEXT_INPUT_PAGE: [u8; 3] = [0x10, 0x12, 0x16];

/// `default.css`'s text colour in an input.
pub const UI_TEXT_INPUT_TEXT: [u8; 3] = [0xe6, 0xe9, 0xef];

/// `default.css`'s selection highlight.
pub const UI_TEXT_INPUT_SELECTION: [u8; 3] = [0x2f, 0x5a, 0x9e];

/// `default.css`'s caret, which is also its accent.
pub const UI_TEXT_INPUT_CARET: [u8; 3] = [0x3d, 0x8b, 0xfd];

/// `default.css`'s placeholder colour.
pub const UI_TEXT_INPUT_PLACEHOLDER: [u8; 3] = [0x7a, 0x81, 0x90];

/// `default.css`'s horizontal inset from an input's border box to its text: a
/// one-pixel border and four pixels of padding.
pub const UI_TEXT_INPUT_INSET_X: f32 = 5.0;

/// `default.css`'s vertical inset from an input's border box to its text: a
/// one-pixel border and two pixels of padding.
pub const UI_TEXT_INPUT_INSET_Y: f32 = 3.0;

/// `default.css`'s caret width, in pixels.
pub const UI_TEXT_INPUT_CARET_WIDTH: f32 = 1.0;

/// The line typed into `#long`: wider than the input, so it scrolls.
pub const UI_TEXT_INPUT_LONG: &str = "the quick brown fox jumps over";

/// `#picked`'s value.
pub const UI_TEXT_INPUT_PICKED: &str = "select me";

/// The characters of [`UI_TEXT_INPUT_PICKED`] the drag selects, as a range of
/// `char` positions: `lect`.
pub const UI_TEXT_INPUT_PICK: (usize, usize) = (2, 6);

/// `#empty`'s placeholder.
pub const UI_TEXT_INPUT_PLACEHOLDER_TEXT: &str = "name";

/// `#secret`'s value, drawn masked.
pub const UI_TEXT_INPUT_SECRET: &str = "hunter2";

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The scene's stylesheet: the page's layout and fill. Every input's own look
/// is `default.css`'s.
#[must_use]
pub fn ui_text_input_css() -> String {
    format!(
        "\
.page {{
  flex-direction: column;
  align-items: flex-start;
  padding: 6px;
  gap: 6px;
  background: {page};
}}
",
        page = hex(UI_TEXT_INPUT_PAGE),
    )
}

/// One scripted frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Input {
    /// A navigation input, the pointer away from everything.
    Nav(NavInput),
    /// [`UI_TEXT_INPUT_LONG`] typed.
    Type,
    /// The pointer held down on `#picked` at this caret stop.
    PressPicked(usize),
}

/// The scripted input, one frame each after the frame that first lays the page
/// out; see the module docs.
const SCRIPT: [Input; 5] = [
    Input::Nav(NavInput::NAVIGATION),
    Input::Nav(NavInput::ACCEPT),
    Input::Type,
    Input::PressPicked(UI_TEXT_INPUT_PICK.0),
    Input::PressPicked(UI_TEXT_INPUT_PICK.1),
];

/// Where every part of the scene was laid out in its last frame, in screen
/// pixels, each as the `(min, max)` of its border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiTextInputLayout {
    /// The engaged input holding the long line.
    pub long: (Vec2, Vec2),
    /// The input a drag is selecting in.
    pub picked: (Vec2, Vec2),
    /// The empty input showing its placeholder.
    pub empty: (Vec2, Vec2),
    /// The masked input.
    pub secret: (Vec2, Vec2),
}

/// The keys of one frame's inputs.
#[derive(Clone, Copy, Debug)]
struct Parts {
    long: NodeKey,
    picked: NodeKey,
    empty: NodeKey,
    secret: NodeKey,
}

/// The values the inputs edit, carried from frame to frame.
struct Values {
    long: String,
    picked: String,
    empty: String,
    secret: String,
}

fn frame(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    text: TextInput,
    values: &mut Values,
    extent: (u32, u32),
) -> Parts {
    ui.begin_frame_with(pointer, nav);
    ui.set_text_input(text);
    let size = [
        Declaration::Width(LengthAuto::Px(extent.0 as f32)),
        Declaration::Height(LengthAuto::Px(extent.1 as f32)),
    ];
    let mut parts = None;
    ui.block(".page", &size, |ui| {
        let long = ui.text_input("#long", &mut values.long).key;
        let picked = ui.text_input("#picked", &mut values.picked).key;
        let empty = ui
            .text_input_with(
                "#empty",
                &mut values.empty,
                TextInputOptions {
                    placeholder: UI_TEXT_INPUT_PLACEHOLDER_TEXT,
                    masked: false,
                },
            )
            .key;
        let secret = ui
            .text_input_with(
                "#secret",
                &mut values.secret,
                TextInputOptions {
                    placeholder: "",
                    masked: true,
                },
            )
            .key;
        parts = Some(Parts {
            long,
            picked,
            empty,
            secret,
        });
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::new(extent.0 as f32, extent.1 as f32)),
        &FontAtlas::built_in(),
    );
    parts.expect("the page was built")
}

/// Runs the script and returns the tree after its last frame, the keys of that
/// frame, and the values the inputs were left with.
fn build(extent: (u32, u32)) -> (Ui, Parts, Values) {
    let mut ui = Ui::new();
    ui.add_stylesheet("ui_text_input.css", &ui_text_input_css());
    let mut values = Values {
        long: String::new(),
        picked: UI_TEXT_INPUT_PICKED.to_owned(),
        empty: String::new(),
        secret: UI_TEXT_INPUT_SECRET.to_owned(),
    };
    let away = PointerInput::hovering(Vec2::splat(-1.0));
    // No time passes, so the caret never blinks off.
    let still = TextInput {
        dt: Duration::ZERO,
        ..TextInput::default()
    };
    let mut parts = frame(
        &mut ui,
        away,
        NavInput::default(),
        still.clone(),
        &mut values,
        extent,
    );
    for input in SCRIPT {
        let (pointer, nav, text) = match input {
            Input::Nav(nav) => (away, nav, still.clone()),
            Input::Type => (
                away,
                NavInput::NAVIGATION,
                TextInput {
                    edits: vec![Edit::Insert(UI_TEXT_INPUT_LONG.to_owned())],
                    ..still.clone()
                },
            ),
            Input::PressPicked(stop) => {
                let (min, max) = ui.rect(parts.picked).expect("laid out");
                // A pixel past the stop, so the nearest stop is this one.
                let x = min.x + UI_TEXT_INPUT_INSET_X + stop as f32 * GLYPH_ADVANCE + 1.0;
                let pointer = PointerInput {
                    pos: Vec2::new(x, (min.y + max.y) * 0.5),
                    down: true,
                    released: false,
                };
                (pointer, NavInput::default(), still.clone())
            }
        };
        parts = frame(&mut ui, pointer, nav, text, &mut values, extent);
    }
    (ui, parts, values)
}

/// The layout of [`Scene::UiTextInput`](super::Scene::UiTextInput) in an
/// `extent`-sized frame, as its last frame laid it out.
#[must_use]
pub fn ui_text_input_layout(extent: (u32, u32)) -> UiTextInputLayout {
    let (ui, parts, _) = build(extent);
    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    UiTextInputLayout {
        long: rect(parts.long),
        picked: rect(parts.picked),
        empty: rect(parts.empty),
        secret: rect(parts.secret),
    }
}

/// The scene's draw list for an `extent`-sized frame: the script's last frame.
#[must_use]
pub fn ui_text_input_draw_list(extent: (u32, u32)) -> DrawList {
    let (ui, _, _) = build(extent);
    let mut list = DrawList::new();
    ui.emit(&mut list);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw_list::DrawCommand;

    /// The extent every UI golden is blessed at.
    const EXTENT: (u32, u32) = (256, 192);

    /// Whether a linear-light colour from the draw list is `srgb`, to within a
    /// byte.
    fn is(color: [f32; 4], srgb: [u8; 3]) -> bool {
        let close = |linear: f32, byte: u8| {
            let encoded = if linear <= 0.003_130_8 {
                linear * 12.92
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            ((encoded * 255.0).round() as i32 - i32::from(byte)).abs() <= 1
        };
        close(color[0], srgb[0]) && close(color[1], srgb[1]) && close(color[2], srgb[2])
    }

    /// **The script leaves every input where the claims need it**: `#long`
    /// engaged with the whole line typed and its caret at the end, scrolled;
    /// `#picked` holding the press with the pick selected and its text
    /// untouched; and the stylesheet parsed cleanly.
    #[test]
    fn the_script_leaves_every_input_where_the_claims_need_it() {
        let logs = crcbl_core::log::capture();
        let (ui, parts, values) = build(EXTENT);
        assert!(
            logs.records().is_empty(),
            "the scene reported {:#?}",
            logs.records()
        );
        assert_eq!(values.long, UI_TEXT_INPUT_LONG);
        assert_eq!(values.picked, UI_TEXT_INPUT_PICKED);
        assert_eq!(ui.engaged(), Some(parts.long));
        let end = UI_TEXT_INPUT_LONG.chars().count();
        assert_eq!(ui.text_caret(parts.long), Some((end, end)));
        assert!(
            ui.scroll_offset_of(parts.long).x > 0.0,
            "the long line did not scroll"
        );
        assert_eq!(
            ui.text_caret(parts.picked),
            Some((UI_TEXT_INPUT_PICK.1, UI_TEXT_INPUT_PICK.0)),
            "the drag did not select the pick"
        );
    }

    /// **The constants the frame is measured against are the colours the
    /// frame draws**: one selection fill, one caret, the placeholder's text in
    /// its colour and every other string in the text colour.
    #[test]
    fn the_scenes_colours_are_the_ones_drawn() {
        let list = ui_text_input_draw_list(EXTENT);
        let rects = |srgb| {
            list.commands()
                .iter()
                .filter(|command| matches!(command, DrawCommand::Rect { color, .. } if is(*color, srgb)))
                .count()
        };
        assert_eq!(rects(UI_TEXT_INPUT_SELECTION), 1, "not one selection fill");
        let carets: Vec<f32> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, color } if is(*color, UI_TEXT_INPUT_CARET) => {
                    Some(max.x - min.x)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            carets,
            [UI_TEXT_INPUT_CARET_WIDTH],
            "not one caret of its width"
        );
        let texts: Vec<(&str, bool)> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, color, .. } => {
                    Some((text.as_str(), is(*color, UI_TEXT_INPUT_PLACEHOLDER)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            [
                (UI_TEXT_INPUT_LONG, false),
                (UI_TEXT_INPUT_PICKED, false),
                (UI_TEXT_INPUT_PLACEHOLDER_TEXT, true),
                ("*******", false),
            ]
        );
        for command in list.commands() {
            if let DrawCommand::Text { text, color, .. } = command
                && *text != UI_TEXT_INPUT_PLACEHOLDER_TEXT
            {
                assert!(
                    is(*color, UI_TEXT_INPUT_TEXT),
                    "`{text}` is not in the text colour"
                );
            }
        }
    }
}
