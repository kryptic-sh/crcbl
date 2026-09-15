//! [`Scene::UiWidgets`](super::Scene::UiWidgets)'s content: the widget set —
//! `docs/plan/07-ui-debug.md` rung 7 — every widget on one page, styled by the
//! engine's `default.css` and driven by a scripted pointer and pad, so that
//! what the widgets promise can be read back off the frame.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─ .page ────────────────────────────────────────┐
//!   │ ┌─ #left ─────────────┐ ┌─ #right: #panes ────┐ │
//!   │ │ [OK] [x] Mute       │ │ - root              │ │
//!   │ │ #pitch  ▓▓▓░░░░░░░  │ │     a               │ │
//!   │ │╔#volume ▓▓▓▓▓░░░░░╗ │ │   + b               │ │  ← engaged,
//!   │ │ #gain   4.0         │ ├──── divider ────────┤ │    ringed
//!   │ │ - Open              │ │ ┌─ #items ────────┐ │ │
//!   │ │   ▒▒▒▒ body ▒▒▒▒    │ │ │ row 6 (stripes) │ │ │
//!   │ │ + Shut              │ │ │ ...             │ │ │
//!   │ │ ████ #after ████    │ │ └─────────────────┘ │ │
//!   │ └─────────────────────┘ └─────────────────────┘ │
//!   └────────────────────────────────────────────────┘
//! ```
//!
//! The script, one frame each: a click checks `#mute`; a drag moves `#gain`;
//! a click opens `#open`; a drag pulls the divider up; a click opens the
//! tree's root; the pad speaks, and next walks focus from the root through its
//! children and the divider into the list, where steps down take it to
//! [`UI_WIDGETS_LIST_TARGET`], scrolling the list; then a click engages
//! `#volume` at a fifth of its track, and steps right move it to
//! [`UI_WIDGETS_VOLUME`].
//!
//! * **The two sliders** start equal, and only `#volume` is ever engaged:
//!   its fill against `#pitch`'s is the engaged slider having moved and the
//!   other not.
//! * **The shut header** is never opened, and `#after` follows it: its fill
//!   starting one column gap below the header's bottom edge, with one body's
//!   worth of body fill on the frame, is a closed body taking no layout.
//! * **The list's rows** each hold a stripe as wide as its index times
//!   [`UI_WIDGETS_STRIPE_STEP`], coloured by the index's parity, so every
//!   pixel row of the view names the list row the offset put there.
//!
//! Every colour and length the frame is measured against is a constant here.
//! The widgets' own colours are `default.css`'s, and the tests below hold the
//! constants that name them to the resolved style.

use glam::Vec2;

use crate::ui::draw_list::DrawList;
use crate::ui::style::Declaration;
use crate::ui::text::FontAtlas;
use crate::ui::tree::{AvailableSpace, LengthAuto, NavInput, NodeKey, SplitAxis, Ui};
use crate::ui::widget::PointerInput;

/// The page's fill, as the stylesheet writes it: sRGB bytes.
pub const UI_WIDGETS_PAGE: [u8; 3] = [0x10, 0x12, 0x16];

/// `default.css`'s accent: a slider's fill and a checked box's mark.
pub const UI_WIDGETS_ACCENT: [u8; 3] = [0x3d, 0x8b, 0xfd];

/// `default.css`'s focus ring.
pub const UI_WIDGETS_RING: [u8; 3] = [0xf5, 0xc4, 0x00];

/// `default.css`'s collapsing header fill, closed.
pub const UI_WIDGETS_HEADER: [u8; 3] = [0x2a, 0x30, 0x3c];

/// The open header's body block fill.
pub const UI_WIDGETS_BODY: [u8; 3] = [0x70, 0x30, 0x90];

/// The fill of the block after the shut header.
pub const UI_WIDGETS_AFTER: [u8; 3] = [0xd0, 0x60, 0x20];

/// An even row's stripe.
pub const UI_WIDGETS_STRIPE_EVEN: [u8; 3] = [0x20, 0x80, 0x50];

/// An odd row's stripe.
pub const UI_WIDGETS_STRIPE_ODD: [u8; 3] = [0x80, 0x80, 0x20];

/// How much wider each row's stripe is than the row before's, in pixels: row
/// `n`'s stripe is `n` times this wide, so no two rows draw the same stripe.
pub const UI_WIDGETS_STRIPE_STEP: f32 = 4.0;

/// The gap between the left column's widgets, in pixels.
pub const UI_WIDGETS_COLUMN_GAP: f32 = 3.0;

/// The body block's height, in pixels.
pub const UI_WIDGETS_BODY_HEIGHT: f32 = 10.0;

/// A list row's height, in pixels.
pub const UI_WIDGETS_ROW_HEIGHT: f32 = 16.0;

/// How many rows the list holds.
pub const UI_WIDGETS_ROWS: usize = 1000;

/// The row the pad walks focus down to.
pub const UI_WIDGETS_LIST_TARGET: usize = 12;

/// Where both sliders start, on their `0..=10` range.
pub const UI_WIDGETS_SLIDER_START: f32 = 3.0;

/// Where the script leaves `#volume`: the click's value and three steps.
pub const UI_WIDGETS_VOLUME: f32 = 5.0;

/// How far the script drags the divider, in pixels: up.
pub const UI_WIDGETS_DIVIDER_DRAG: f32 = -20.0;

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The scene's stylesheet: the page's layout and the fills the claims read.
/// Every widget's own look is `default.css`'s.
#[must_use]
pub fn ui_widgets_css() -> String {
    format!(
        "\
.page {{
  padding: 6px;
  gap: 6px;
  background: {page};
}}
#left {{
  flex-direction: column;
  gap: {column_gap}px;
  width: 120px;
  flex-shrink: 0;
}}
#actions {{
  gap: 6px;
  align-items: center;
}}
#right {{
  flex-grow: 1;
  flex-direction: column;
}}
.body-fill {{
  height: {body_height}px;
  background: {body};
}}
#after {{
  height: 8px;
  background: {after};
}}
#items {{
  flex-grow: 1;
}}
.stripe {{
  flex-shrink: 0;
  align-self: stretch;
  background: {odd};
}}
.stripe.even {{
  background: {even};
}}
",
        page = hex(UI_WIDGETS_PAGE),
        column_gap = UI_WIDGETS_COLUMN_GAP,
        body_height = UI_WIDGETS_BODY_HEIGHT,
        body = hex(UI_WIDGETS_BODY),
        after = hex(UI_WIDGETS_AFTER),
        odd = hex(UI_WIDGETS_STRIPE_ODD),
        even = hex(UI_WIDGETS_STRIPE_EVEN),
    )
}

/// One scripted frame: where the pointer is and what the pad does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Input {
    /// The pointer held at a point `offset` from the centre of `part` as last
    /// laid out, or released there.
    Pointer {
        part: Part,
        offset: Vec2,
        down: bool,
    },
    /// A navigation input, the pointer away from everything.
    Nav(NavInput),
}

/// The parts the script points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Mute,
    Gain,
    Open,
    Divider,
    Root,
    Volume,
}

const fn press(part: Part, x: f32, y: f32) -> Input {
    Input::Pointer {
        part,
        offset: Vec2::new(x, y),
        down: true,
    }
}

const fn release(part: Part, x: f32, y: f32) -> Input {
    Input::Pointer {
        part,
        offset: Vec2::new(x, y),
        down: false,
    }
}

/// The script's length.
const SCRIPT_FRAMES: usize = 34;

/// The scripted input, one frame each after the frame that first lays the page
/// out; see the module docs. A drag's later frames point at where the drag
/// began — the part moves under it — so each names the same part's centre as
/// its first frame, plus the distance dragged.
const SCRIPT: [Input; SCRIPT_FRAMES] = {
    use crate::ui::tree::Direction;
    const DOWN: Input = Input::Nav(NavInput::toward(Direction::Down));
    const NEXT: Input = Input::Nav(NavInput::NEXT);
    const RIGHT: Input = Input::Nav(NavInput::toward(Direction::Right));
    [
        press(Part::Mute, 0.0, 0.0),
        release(Part::Mute, 0.0, 0.0),
        press(Part::Gain, 0.0, 0.0),
        press(Part::Gain, 30.0, 0.0),
        release(Part::Gain, 30.0, 0.0),
        press(Part::Open, 0.0, 0.0),
        release(Part::Open, 0.0, 0.0),
        press(Part::Divider, 0.0, 0.0),
        press(Part::Divider, 0.0, UI_WIDGETS_DIVIDER_DRAG),
        release(Part::Divider, 0.0, UI_WIDGETS_DIVIDER_DRAG),
        press(Part::Root, 0.0, 0.0),
        release(Part::Root, 0.0, 0.0),
        Input::Nav(NavInput::NAVIGATION),
        // The root's two children, the divider, then the list's first row.
        NEXT,
        NEXT,
        NEXT,
        NEXT,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        DOWN,
        // A fifth of the way along `#volume`'s track engages it at 2.
        press(Part::Volume, -0.3, 0.0),
        release(Part::Volume, -0.3, 0.0),
        RIGHT,
        RIGHT,
        RIGHT,
    ]
};

/// Where every part of the scene was laid out in its last frame, in screen
/// pixels, each as the `(min, max)` of its border box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiWidgetsLayout {
    /// The checked checkbox.
    pub mute: (Vec2, Vec2),
    /// The slider nothing engaged.
    pub pitch: (Vec2, Vec2),
    /// The slider the script engaged and moved.
    pub volume: (Vec2, Vec2),
    /// The open header's row.
    pub open: (Vec2, Vec2),
    /// The shut header's row.
    pub shut: (Vec2, Vec2),
    /// The block after the shut header.
    pub after: (Vec2, Vec2),
    /// The virtualized list.
    pub items: (Vec2, Vec2),
    /// How far the list is scrolled, in pixels.
    pub items_offset: f32,
}

/// The keys the claims and the script need, from one frame.
#[derive(Clone, Copy, Debug)]
struct Parts {
    mute: NodeKey,
    gain: NodeKey,
    pitch: NodeKey,
    volume: NodeKey,
    open: NodeKey,
    shut: NodeKey,
    after: NodeKey,
    root: NodeKey,
    divider: NodeKey,
    items: NodeKey,
}

/// The values the widgets edit, carried from frame to frame.
struct Values {
    mute: bool,
    pitch: f32,
    volume: f32,
    gain: f32,
}

impl Parts {
    const fn of(&self, part: Part) -> NodeKey {
        match part {
            Part::Mute => self.mute,
            Part::Gain => self.gain,
            Part::Open => self.open,
            Part::Divider => self.divider,
            Part::Root => self.root,
            Part::Volume => self.volume,
        }
    }
}

fn frame(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    values: &mut Values,
    extent: (u32, u32),
) -> Parts {
    ui.begin_frame_with(pointer, nav);
    let size = [
        Declaration::Width(LengthAuto::Px(extent.0 as f32)),
        Declaration::Height(LengthAuto::Px(extent.1 as f32)),
    ];
    let mut parts = None;
    ui.block(".page", &size, |ui| {
        let mut left = None;
        ui.block("#left", &[], |ui| {
            let mut actions = None;
            ui.block("#actions", &[], |ui| {
                ui.button("#ok", "OK");
                let mute = ui.checkbox("#mute", "Mute", &mut values.mute);
                actions = Some(mute.key);
            });
            let mute = actions.expect("built");
            let pitch = ui.slider("#pitch", &mut values.pitch, 0.0..=10.0, 1.0).key;
            let volume = ui
                .slider("#volume", &mut values.volume, 0.0..=10.0, 1.0)
                .key;
            let gain = ui
                .drag_value("#gain", &mut values.gain, 0.0..=10.0, 0.1, 0.5)
                .key;
            let open = ui
                .collapsing("#open", "Open", |ui| {
                    ui.block(".body-fill", &[], |_| {});
                })
                .key;
            let shut = ui
                .collapsing("#shut", "Shut", |ui| {
                    ui.block(".body-fill", &[], |_| {});
                })
                .key;
            let after = ui.block("#after", &[], |_| {}).key;
            left = Some((mute, pitch, volume, gain, open, shut, after));
        });
        let (mute, pitch, volume, gain, open, shut, after) = left.expect("built");

        let (mut root, mut items) = (None, None);
        let mut divider = None;
        ui.block("#right", &[], |ui| {
            divider = Some(
                ui.split(
                    "#panes",
                    SplitAxis::Column,
                    [30.0, 30.0],
                    |ui| {
                        root = Some(
                            ui.tree_node("#root", "root", |ui| {
                                ui.tree_leaf("#a", "a");
                                ui.tree_node("#b", "b", |ui| {
                                    ui.tree_leaf("#c", "c");
                                });
                            })
                            .key,
                        );
                    },
                    |ui| {
                        items = Some(
                            ui.list(
                                "#items",
                                UI_WIDGETS_ROWS,
                                UI_WIDGETS_ROW_HEIGHT,
                                |ui, index| {
                                    let stripe = if index.is_multiple_of(2) {
                                        ".stripe.even"
                                    } else {
                                        ".stripe"
                                    };
                                    let width = index as f32 * UI_WIDGETS_STRIPE_STEP;
                                    ui.block(
                                        stripe,
                                        &[Declaration::Width(LengthAuto::Px(width))],
                                        |_| {},
                                    );
                                },
                            )
                            .key,
                        );
                    },
                )
                .key,
            );
        });

        parts = Some(Parts {
            mute,
            gain,
            pitch,
            volume,
            open,
            shut,
            after,
            root: root.expect("built"),
            divider: divider.expect("built"),
            items: items.expect("built"),
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
/// frame, and the values the widgets were left with.
fn build(extent: (u32, u32)) -> (Ui, Parts, Values) {
    let mut ui = Ui::new();
    ui.add_stylesheet("ui_widgets.css", &ui_widgets_css());
    let mut values = Values {
        mute: false,
        pitch: UI_WIDGETS_SLIDER_START,
        volume: UI_WIDGETS_SLIDER_START,
        gain: 1.0,
    };
    let away = PointerInput::hovering(Vec2::splat(-1.0));
    let mut parts = frame(&mut ui, away, NavInput::default(), &mut values, extent);
    // A drag is aimed at where its part was when the drag began.
    let mut anchor = None;
    for input in SCRIPT {
        let (pointer, nav) = match input {
            Input::Pointer { part, offset, down } => {
                let (min, max) = ui.rect(parts.of(part)).expect("laid out");
                let centre = *anchor.get_or_insert((min + max) * 0.5);
                let point = if part == Part::Volume {
                    // A share of the track, not pixels: `offset.x` from the
                    // middle as a fraction of the width.
                    centre + Vec2::new(offset.x * (max.x - min.x), 0.0)
                } else {
                    centre + offset
                };
                if !down {
                    anchor = None;
                }
                let pointer = PointerInput {
                    pos: point,
                    down,
                    released: !down,
                };
                (pointer, NavInput::default())
            }
            Input::Nav(nav) => (away, nav),
        };
        parts = frame(&mut ui, pointer, nav, &mut values, extent);
    }
    (ui, parts, values)
}

/// The layout of [`Scene::UiWidgets`](super::Scene::UiWidgets) in an
/// `extent`-sized frame, as its last frame laid it out.
#[must_use]
pub fn ui_widgets_layout(extent: (u32, u32)) -> UiWidgetsLayout {
    let (ui, parts, _) = build(extent);
    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    UiWidgetsLayout {
        mute: rect(parts.mute),
        pitch: rect(parts.pitch),
        volume: rect(parts.volume),
        open: rect(parts.open),
        shut: rect(parts.shut),
        after: rect(parts.after),
        items: rect(parts.items),
        items_offset: ui.scroll_offset_of(parts.items).y,
    }
}

/// The scene's draw list for an `extent`-sized frame: the script's last frame.
#[must_use]
pub fn ui_widgets_draw_list(extent: (u32, u32)) -> DrawList {
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

    /// Every filled rectangle of `srgb` on the scene's last frame.
    fn rects_of(srgb: [u8; 3]) -> Vec<(Vec2, Vec2)> {
        ui_widgets_draw_list(EXTENT)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, color } if is(*color, srgb) => Some((*min, *max)),
                _ => None,
            })
            .collect()
    }

    /// The scene is what its claims need before any pixel is read: the script
    /// ends with `#volume` engaged, focused and moved while `#pitch` is where
    /// it started; the box checked, the drag-value dragged, the open header
    /// open and the shut one shut; the list scrolled exactly far enough to show
    /// its target row; and the stylesheet parsed cleanly.
    #[test]
    fn the_script_leaves_every_widget_where_the_claims_need_it() {
        let logs = crcbl_core::log::capture();
        let (ui, parts, values) = build(EXTENT);
        assert!(
            logs.records().is_empty(),
            "the scene reported {:#?}",
            logs.records()
        );
        assert!(values.mute, "the click did not check the box");
        assert_eq!(values.gain, 4.0, "thirty pixels at 0.1 a pixel from 1");
        assert_eq!(
            values.pitch, UI_WIDGETS_SLIDER_START,
            "the slider nothing engaged moved"
        );
        assert_eq!(values.volume, UI_WIDGETS_VOLUME);
        assert_ne!(
            values.volume, values.pitch,
            "the claim would compare equal fills"
        );
        assert_eq!(ui.focused(), Some(parts.volume));
        assert_eq!(ui.engaged(), Some(parts.volume));
        assert!(ui.is_open(parts.open) && !ui.is_open(parts.shut));
        assert!(ui.is_open(parts.root));

        let layout = ui_widgets_layout(EXTENT);
        // The list's view: its border box less `default.css`'s one-pixel border.
        let view = layout.items.1.y - layout.items.0.y - 2.0;
        let reach = (UI_WIDGETS_LIST_TARGET + 1) as f32 * UI_WIDGETS_ROW_HEIGHT - view;
        assert!(
            reach > 0.0,
            "the target row is in view unscrolled, so proves nothing"
        );
        assert_eq!(
            layout.items_offset, reach,
            "the list did not scroll to show its target"
        );
        assert_eq!(
            layout.after.0.y,
            layout.shut.1.y + UI_WIDGETS_COLUMN_GAP,
            "the block after the shut header does not start one gap below the header"
        );
    }

    /// **The constants the frame is measured against are the colours the
    /// frame draws**: `default.css`'s accent fills both sliders and the
    /// checked mark, its ring is the one outline, round `#volume`, and its
    /// header fill is the shut header's — and the open body's fill is drawn
    /// once, for the one body built.
    #[test]
    fn the_scenes_colours_are_the_ones_drawn() {
        let layout = ui_widgets_layout(EXTENT);
        let inside = |(min, max): (Vec2, Vec2), (outer_min, outer_max): (Vec2, Vec2)| {
            min.cmpge(outer_min).all() && max.cmple(outer_max).all()
        };
        let accent = rects_of(UI_WIDGETS_ACCENT);
        for (name, part) in [
            ("#pitch", layout.pitch),
            ("#volume", layout.volume),
            ("#mute", layout.mute),
        ] {
            assert!(
                accent.iter().any(|&fill| inside(fill, part)),
                "no accent fill inside {name}: {accent:?}"
            );
        }
        assert!(
            rects_of(UI_WIDGETS_HEADER).contains(&layout.shut),
            "the shut header is not drawn in the header fill"
        );
        assert_eq!(
            rects_of(UI_WIDGETS_BODY).len(),
            1,
            "not exactly one body drawn"
        );

        let rings: Vec<(Vec2, Vec2)> = ui_widgets_draw_list(EXTENT)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::RectOutline {
                    min, max, color, ..
                } if is(*color, UI_WIDGETS_RING) => Some((*min, *max)),
                _ => None,
            })
            .collect();
        let grow = Vec2::splat(3.0);
        assert_eq!(
            rings,
            [(layout.volume.0 - grow, layout.volume.1 + grow)],
            "not exactly one ring, round the engaged slider"
        );
    }
}
