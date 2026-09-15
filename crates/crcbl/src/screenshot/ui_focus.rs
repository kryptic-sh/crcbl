//! [`Scene::UiFocus`](super::Scene::UiFocus)'s content: focus driven by a
//! scripted pad — `docs/plan/07-ui-debug.md` rung 6 — through a grid of
//! buttons, into a modal that opens over it, and down a scroll list inside the
//! modal, so that what focus promises can be read back off the frame.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─ .page ─────────────────────────────────────┐
//!   │ ┌─ #grid ──────┐   ┌─ #dialog (modal) ────┐ │
//!   │ │ ▢  ▣  ▢      │   │ ┌─ #list (scroll) ─┐ │ │   ▣ `#g1`, which opened
//!   │ │ ▢  ▢  ▢      │   │ │ row 3            │ │ │     the dialog
//!   │ └──────────────┘   │ │ row 4            │ │ │
//!   │                    │ │╔row 5 (target)═╗ │ │ │   ← focused: the one ring
//!   │                    │ └──────────────────┘ │ │     on the frame
//!   │                    │             #close ▢ │ │
//!   │                    └──────────────────────┘ │
//!   └─────────────────────────────────────────────┘
//! ```
//!
//! The script, one [`NavInput`] a frame: the pad speaks and focus lands on
//! `#g0`; right to `#g1`; accept fires `#g1`, which opens the dialog; the pad
//! speaks again and the modal pulls focus to its first row; five steps down
//! reach [`UI_FOCUS_TARGET_ROW`], scrolling the list; and a last step left —
//! toward the grid — which the modal must refuse.
//!
//! * **The ring** is the stylesheet's `.button:focus` outline, and every
//!   focusable node has that rule, so exactly one ring on the frame means
//!   exactly one node shows focus.
//! * **The dialog** is a [`Behavior::MODAL`] scope: the ring inside it after a
//!   step toward the grid is the trap holding. Its close button sits at its
//!   right edge, so nothing inside the dialog lies left of a row either, and
//!   that step has nowhere to go but out.
//! * **The target row** has a fill of its own, and starts below the list's
//!   view: its whole fill on the frame, ringed, is the list having scrolled.
//!
//! Every colour and length the frame is measured against is a constant here,
//! and the stylesheet is written from those constants. Its lengths are not
//! scaled with the frame.

use glam::Vec2;

use crate::ui::draw_list::DrawList;
use crate::ui::text::FontAtlas;
use crate::ui::tree::{AvailableSpace, Behavior, Direction, NavInput, NodeKey, Response, Ui};
use crate::ui::widget::PointerInput;

/// The page's fill, as the stylesheet writes it: sRGB bytes.
pub const UI_FOCUS_PAGE: [u8; 3] = [0x14, 0x18, 0x20];

/// A button's fill: the grid's and the dialog's close button.
pub const UI_FOCUS_BUTTON: [u8; 3] = [0x40, 0x50, 0x70];

/// The dialog's fill.
pub const UI_FOCUS_DIALOG: [u8; 3] = [0x30, 0x28, 0x40];

/// The list's fill, which shows in its padding.
pub const UI_FOCUS_LIST: [u8; 3] = [0x10, 0x30, 0x30];

/// A row's fill.
pub const UI_FOCUS_ROW: [u8; 3] = [0x30, 0x60, 0x60];

/// The target row's fill.
pub const UI_FOCUS_TARGET: [u8; 3] = [0xe0, 0xc0, 0x20];

/// The focus ring's colour.
pub const UI_FOCUS_RING: [u8; 3] = [0xff, 0x30, 0xa0];

/// The focus ring's width, in pixels.
pub const UI_FOCUS_RING_WIDTH: f32 = 2.0;

/// How far outside a node's border box its ring starts, in pixels.
pub const UI_FOCUS_RING_OFFSET: f32 = 1.0;

/// How many rows the list holds.
pub const UI_FOCUS_ROWS: usize = 8;

/// The row the script walks focus down to.
pub const UI_FOCUS_TARGET_ROW: usize = 5;

/// A row's height, in pixels.
pub const UI_FOCUS_ROW_HEIGHT: f32 = 20.0;

/// The list's border-box height, in pixels: three rows and a part.
pub const UI_FOCUS_LIST_HEIGHT: f32 = 72.0;

/// The list's padding on every side, in pixels: room for a row's ring.
pub const UI_FOCUS_LIST_PADDING: f32 = 4.0;

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The scene's stylesheet, written from the constants above.
#[must_use]
pub fn ui_focus_css() -> String {
    format!(
        "\
.page {{
  padding: 8px;
  gap: 8px;
  background: {page};
}}
#grid {{
  flex-direction: column;
  gap: 4px;
}}
.grid-row {{
  gap: 4px;
}}
.button {{
  width: 30px;
  height: 20px;
  background: {button};
  outline-color: {ring};
  outline-offset: {offset}px;
}}
.button:focus {{
  outline-width: {width}px;
}}
#dialog {{
  position: absolute;
  left: 124px;
  top: 8px;
  width: 120px;
  padding: 6px;
  gap: 6px;
  flex-direction: column;
  background: {dialog};
}}
#list {{
  overflow: scroll;
  flex-direction: column;
  height: {list_height}px;
  padding: {list_padding}px;
  background: {list};
}}
.row {{
  width: auto;
  height: {row_height}px;
  flex-shrink: 0;
  background: {row};
}}
.row.target {{
  background: {target};
}}
#close {{
  align-self: flex-end;
}}
",
        page = hex(UI_FOCUS_PAGE),
        button = hex(UI_FOCUS_BUTTON),
        ring = hex(UI_FOCUS_RING),
        offset = UI_FOCUS_RING_OFFSET,
        width = UI_FOCUS_RING_WIDTH,
        dialog = hex(UI_FOCUS_DIALOG),
        list_height = UI_FOCUS_LIST_HEIGHT,
        list_padding = UI_FOCUS_LIST_PADDING,
        list = hex(UI_FOCUS_LIST),
        row_height = UI_FOCUS_ROW_HEIGHT,
        row = hex(UI_FOCUS_ROW),
        target = hex(UI_FOCUS_TARGET),
    )
}

/// The scripted input, one frame each after the frame that first lays the page
/// out; see the module docs.
const SCRIPT: [NavInput; 11] = [
    NavInput::NAVIGATION,
    NavInput::toward(Direction::Right),
    NavInput::ACCEPT,
    NavInput::NAVIGATION,
    NavInput::toward(Direction::Down),
    NavInput::toward(Direction::Down),
    NavInput::toward(Direction::Down),
    NavInput::toward(Direction::Down),
    NavInput::toward(Direction::Down),
    NavInput::toward(Direction::Left),
    NavInput::NAVIGATION,
];

/// Where every part of the scene was laid out in its last frame, in screen
/// pixels, each as the `(min, max)` of its border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiFocusLayout {
    /// The grid button that opened the dialog, and was focused before it.
    pub opener: (Vec2, Vec2),
    /// The modal dialog.
    pub dialog: (Vec2, Vec2),
    /// The scroll list.
    pub list: (Vec2, Vec2),
    /// The row focus was walked to, where the scrolled list drew it.
    pub target: (Vec2, Vec2),
}

/// The keys the scene's claims need, from one frame.
struct Parts {
    opener: Response,
    dialog: Option<NodeKey>,
    list: Option<NodeKey>,
    rows: Vec<NodeKey>,
}

fn frame(ui: &mut Ui, nav: NavInput, open: &mut bool, extent: (u32, u32)) -> Parts {
    ui.begin_frame_with(PointerInput::hovering(Vec2::splat(-1.0)), nav);
    let mut opener = None;
    let mut dialog = None;
    let mut list = None;
    let mut rows = Vec::new();
    let size = [
        crate::ui::style::Declaration::Width(crate::ui::tree::LengthAuto::Px(extent.0 as f32)),
        crate::ui::style::Declaration::Height(crate::ui::tree::LengthAuto::Px(extent.1 as f32)),
    ];
    ui.block(".page", &size, |ui| {
        ui.block("#grid", &[], |ui| {
            for (row, ids) in [["#g0", "#g1", "#g2"], ["#g3", "#g4", "#g5"]]
                .iter()
                .enumerate()
            {
                ui.block_keyed(row, ".grid-row", &[], |ui| {
                    for id in ids {
                        let selector = format!("{id}.button");
                        let response = ui.block_with(&selector, &[], Behavior::BUTTON, |_| {});
                        if *id == "#g1" {
                            opener = Some(response);
                        }
                    }
                });
            }
        });
        let opened = opener.expect("the grid was built");
        *open |= opened.clicked;
        if *open {
            dialog = Some(
                ui.block_with("#dialog", &[], Behavior::MODAL, |ui| {
                    list = Some(
                        ui.block("#list", &[], |ui| {
                            for index in 0..UI_FOCUS_ROWS {
                                let selector = if index == UI_FOCUS_TARGET_ROW {
                                    ".button.row.target"
                                } else {
                                    ".button.row"
                                };
                                rows.push(
                                    ui.block_keyed_with(
                                        index,
                                        selector,
                                        &[],
                                        Behavior::BUTTON,
                                        |_| {},
                                    )
                                    .key,
                                );
                            }
                        })
                        .key,
                    );
                    ui.block_with("#close.button", &[], Behavior::BUTTON, |_| {});
                })
                .key,
            );
        }
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::new(extent.0 as f32, extent.1 as f32)),
        &FontAtlas::built_in(),
    );
    Parts {
        opener: opener.expect("the grid was built"),
        dialog,
        list,
        rows,
    }
}

/// Runs the script and returns the tree after its last frame, the keys of that
/// frame, and its layout.
fn build(extent: (u32, u32)) -> (Ui, Parts, UiFocusLayout) {
    let mut ui = Ui::new();
    ui.add_stylesheet("ui_focus.css", &ui_focus_css());
    let mut open = false;
    let mut parts = frame(&mut ui, NavInput::default(), &mut open, extent);
    for nav in SCRIPT {
        parts = frame(&mut ui, nav, &mut open, extent);
    }
    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    let layout = UiFocusLayout {
        opener: rect(parts.opener.key),
        dialog: rect(parts.dialog.expect("the script opens the dialog")),
        list: rect(parts.list.expect("the script opens the dialog")),
        target: rect(parts.rows[UI_FOCUS_TARGET_ROW]),
    };
    (ui, parts, layout)
}

/// The layout of [`Scene::UiFocus`](super::Scene::UiFocus) in an
/// `extent`-sized frame, as its last frame laid it out.
#[must_use]
pub fn ui_focus_layout(extent: (u32, u32)) -> UiFocusLayout {
    build(extent).2
}

/// The scene's draw list for an `extent`-sized frame: the script's last frame.
#[must_use]
pub fn ui_focus_draw_list(extent: (u32, u32)) -> DrawList {
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

    /// The scene is what its claims need before any pixel is read: the script
    /// ends with focus on the target row inside the dialog and nothing
    /// engaged, the list scrolled to show that row and no further than its
    /// reach, the row lies below the list's view unscrolled, the stylesheet
    /// parsed cleanly, and exactly one outline is drawn — round the target.
    #[test]
    fn the_script_leaves_focus_where_the_claims_need_it() {
        let logs = crcbl_core::log::capture();
        let (ui, parts, layout) = build(EXTENT);
        assert!(
            logs.records().is_empty(),
            "the scene reported {:#?}",
            logs.records()
        );
        assert_eq!(ui.focused(), Some(parts.rows[UI_FOCUS_TARGET_ROW]));
        assert_eq!(ui.engaged(), None);

        let inside = |(min, max): (Vec2, Vec2), (inner_min, inner_max): (Vec2, Vec2)| {
            inner_min.cmpge(min).all() && inner_max.cmple(max).all()
        };
        let view = (
            layout.list.0 + Vec2::splat(UI_FOCUS_LIST_PADDING),
            layout.list.1 - Vec2::splat(UI_FOCUS_LIST_PADDING),
        );
        assert!(
            inside(view, layout.target),
            "the target {:?} is outside the list's view {view:?}",
            layout.target
        );
        assert!(inside(layout.dialog, layout.list));
        let unscrolled_bottom = view.0.y + (UI_FOCUS_TARGET_ROW + 1) as f32 * UI_FOCUS_ROW_HEIGHT;
        assert!(
            unscrolled_bottom > view.1.y,
            "the target row would be in view without scrolling, so its being there proves nothing"
        );

        let ring = |color: [f32; 4]| {
            let [r, g, b] = UI_FOCUS_RING;
            let close = |linear: f32, byte: u8| {
                let encoded = if linear <= 0.003_130_8 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1.0 / 2.4) - 0.055
                };
                ((encoded * 255.0).round() as i32 - i32::from(byte)).abs() <= 1
            };
            close(color[0], r) && close(color[1], g) && close(color[2], b)
        };
        let outlines: Vec<(Vec2, Vec2)> = ui_focus_draw_list(EXTENT)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::RectOutline {
                    min, max, color, ..
                } if ring(*color) => Some((*min, *max)),
                _ => None,
            })
            .collect();
        let grow = Vec2::splat(UI_FOCUS_RING_OFFSET + UI_FOCUS_RING_WIDTH);
        assert_eq!(
            outlines,
            [(layout.target.0 - grow, layout.target.1 + grow)],
            "not exactly one ring, round the target"
        );
    }
}
