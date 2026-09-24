//! [`Scene::UiLayout`](super::Scene::UiLayout)'s content: the editor-grade
//! surfaces of UI rung 8a — a virtualized outliner, a
//! tab strip and a dockable splitter layout — on one page, styled by the
//! engine's `default.css` and driven by a scripted pointer and pad, so that
//! what each promises can be read back off the frame.
//!
//! A scene of its own rather than more of `ui_widgets`: that page is already
//! two full columns of widgets at this extent, and all three surfaces here want
//! room — the outliner needs a view several rows tall to scroll inside, the
//! dock needs both of its dividers reachable, and the tabs need a pane wide
//! enough to tell one tab's fill from another's.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─ .page ────────────────────────────────────────┐
//!   │ ┌─ "outline" ───┐ ┌─ "views" ─────────────────┐ │
//!   │ │ - root        │ │ [Set][Lib][Log]           │ │
//!   │ │   ▓▓ leaf     │ │ ┌─ the showing pane ────┐ │ │
//!   │ │   ▓▓▓▓ leaf   │ │ │ ░░░░ its own fill ░░░ │ │ │
//!   │ │ ▒▒ SELECTED   │ │ └───────────────────────┘ │ │
//!   │ │   ▓ leaf      │ ├──────── divider ──────────┤ │
//!   │ │   ...         │ │ ┌─ "log" ───────────────┐ │ │
//!   │ └───────────────┘ └───────────────────────────┘ │
//!   └────────────────────────────────────────────────┘
//! ```
//!
//! The outliner starts with [`UI_LAYOUT_NESTED`] expanded, so the model has a
//! third level whichever way the root is opened. The script, one frame each:
//! the pad speaks and focus lands on the first row; **right** opens it, the
//! WAI-ARIA tree view rule on the frame; a drag pulls the dock's outer divider
//! left by [`UI_LAYOUT_DIVIDER_DRAG`]; a click on row
//! [`UI_LAYOUT_CLICKED_ROW`] focuses and selects it; steps down walk focus to
//! [`UI_LAYOUT_SELECTED_ROW`], scrolling the outliner; accept selects the row
//! focus landed on; and a click shows the middle tab.
//!
//! * **The outliner's rows** each hold a stripe whose width is its item's, from
//!   [`ui_layout_rows`], whose colour is that item's parity and whose left edge
//!   is that row's depth times [`ui_layout_indent`] — so every pixel row of the
//!   view names the model row the offset put there, at the depth the model puts
//!   it at.
//! * **The selected row** is the only row `default.css` fills in
//!   [`UI_LAYOUT_SELECTED`]; a row that merely holds focus is ringed, not
//!   filled.
//! * **Each tab's pane** draws a fill of its own, from [`UI_LAYOUT_TAB_FILLS`],
//!   so the two panes that are not showing are absent from the frame rather
//!   than merely hidden.
//! * **The dock's panes** move together: the drag is read as the gap between
//!   the outliner pane's right edge and the tab pane's left edge sitting one
//!   divider wide, both away from the even share by what was dragged.
//!
//! Every colour and length the frame is measured against is a constant here.
//! The widgets' own colours are `default.css`'s, and the tests below hold the
//! constants that name them to the resolved style.

use glam::Vec2;

use crate::ui::draw_list::DrawList;
use crate::ui::style::Declaration;
use crate::ui::text::FontAtlas;
use crate::ui::tree::{
    AvailableSpace, DockLayout, LengthAuto, NavInput, NodeKey, OutlinerBuilder, OutlinerId,
    OutlinerOptions, OutlinerState, SelectMode, SplitAxis, Ui,
};
use crate::ui::widget::PointerInput;

/// The page's fill, as the stylesheet writes it: sRGB bytes.
pub const UI_LAYOUT_PAGE: [u8; 3] = [0x10, 0x12, 0x16];

/// `default.css`'s outliner selection fill.
pub const UI_LAYOUT_SELECTED: [u8; 3] = [0x2f, 0x5a, 0x9e];

/// `default.css`'s accent: the showing tab's fill.
pub const UI_LAYOUT_ACCENT: [u8; 3] = [0x3d, 0x8b, 0xfd];

/// The fill each tab's pane draws, in the order [`UI_LAYOUT_TABS`] names them.
/// Only the showing tab's is on the frame.
pub const UI_LAYOUT_TAB_FILLS: [[u8; 3]; 3] =
    [[0x70, 0x30, 0x90], [0x20, 0x80, 0x50], [0xd0, 0x60, 0x20]];

/// The fill the dock's bottom pane draws.
pub const UI_LAYOUT_LOG_FILL: [u8; 3] = [0x80, 0x30, 0x40];

/// An even-numbered item's stripe.
pub const UI_LAYOUT_STRIPE_EVEN: [u8; 3] = [0x30, 0x90, 0xa0];

/// An odd-numbered item's stripe.
pub const UI_LAYOUT_STRIPE_ODD: [u8; 3] = [0xa0, 0x90, 0x30];

/// How much wider each step of an item's stripe is, in pixels: item `n` draws
/// `n % UI_LAYOUT_STRIPE_CYCLE + 1` steps of it.
pub const UI_LAYOUT_STRIPE_STEP: f32 = 3.0;

/// How many distinct stripe widths there are; see [`UI_LAYOUT_STRIPE_STEP`].
pub const UI_LAYOUT_STRIPE_CYCLE: u64 = 7;

/// An outliner row's height, in pixels.
pub const UI_LAYOUT_ROW_HEIGHT: f32 = 12.0;

/// How many leaves the outliner's root holds.
pub const UI_LAYOUT_LEAVES: u64 = 400;

/// The leaf the tree makes a branch of instead, so the model has a third level.
///
/// Inside the window the scroll leaves showing, so the frame carries rows of
/// two depths and the indentation is on the picture.
pub const UI_LAYOUT_NESTED: u64 = 30;

/// The first id of that branch's children.
pub const UI_LAYOUT_NESTED_BASE: u64 = 10_000;

/// How many children that branch holds.
pub const UI_LAYOUT_NESTED_KIDS: u64 = 3;

/// The flattened row the script's first click lands on.
pub const UI_LAYOUT_CLICKED_ROW: usize = 1;

/// The flattened row the pad walks focus to and accept selects.
///
/// Far enough down that showing it scrolls the view past its own height, so a
/// window built at the wrong offset shows nothing the claim expects rather than
/// the same rows one place over.
pub const UI_LAYOUT_SELECTED_ROW: usize = 40;

/// The tab strip's titles.
pub const UI_LAYOUT_TABS: [&str; 3] = ["Set", "Lib", "Log"];

/// The tab the script shows.
pub const UI_LAYOUT_SHOWN_TAB: usize = 1;

/// How far the script drags the dock's outer divider, in pixels: left.
pub const UI_LAYOUT_DIVIDER_DRAG: f32 = -30.0;

/// Every pane's minimum length along its split, in pixels.
pub const UI_LAYOUT_PANE_MIN: [f32; 2] = [40.0, 30.0];

/// The page's padding round the dock, in pixels.
pub const UI_LAYOUT_PADDING: f32 = 4.0;

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The scene's stylesheet: the page's layout and the fills the claims read.
/// Every widget's own look is `default.css`'s.
#[must_use]
pub fn ui_layout_css() -> String {
    let mut css = format!(
        "\
.page {{
  padding: {padding}px;
  background: {page};
}}
outliner {{
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
.tab-fill {{
  flex-grow: 1;
  align-self: stretch;
}}
#log-fill {{
  flex-grow: 1;
  align-self: stretch;
  background: {log};
}}
",
        padding = UI_LAYOUT_PADDING,
        page = hex(UI_LAYOUT_PAGE),
        odd = hex(UI_LAYOUT_STRIPE_ODD),
        even = hex(UI_LAYOUT_STRIPE_EVEN),
        log = hex(UI_LAYOUT_LOG_FILL),
    );
    for (index, fill) in UI_LAYOUT_TAB_FILLS.iter().enumerate() {
        css.push_str(&format!(
            ".tab-fill.fill-{index} {{ background: {}; }}\n",
            hex(*fill)
        ));
    }
    css
}

/// The tree the outliner shows: one root branch over [`UI_LAYOUT_LEAVES`]
/// leaves, one of which is a branch of its own so that the model has a third
/// level. The ids are what the stripes are drawn from.
fn tree(out: &mut OutlinerBuilder<'_>) {
    out.branch(OutlinerId(0), |out| {
        for leaf in 1..=UI_LAYOUT_LEAVES {
            if leaf == UI_LAYOUT_NESTED {
                out.branch(OutlinerId(leaf), |out| {
                    for kid in 0..UI_LAYOUT_NESTED_KIDS {
                        out.leaf(OutlinerId(UI_LAYOUT_NESTED_BASE + kid));
                    }
                });
            } else {
                out.leaf(OutlinerId(leaf));
            }
        }
    });
}

/// The outliner's flattened rows as `(item, depth)`, in the order the scene's
/// last frame shows them: the model the stripes, the indentation and the scroll
/// claim are read against.
#[must_use]
pub fn ui_layout_rows() -> Vec<(u64, u16)> {
    let mut state = OutlinerState::new();
    for id in [0, UI_LAYOUT_NESTED] {
        state.set_expanded(OutlinerId(id), true);
    }
    state.flatten(tree);
    state
        .rows()
        .iter()
        .map(|row| (row.id.0, row.depth))
        .collect()
}

/// How far a row of `depth` is indented from the outliner's content box, in
/// pixels: [`OutlinerOptions`]' default step per level.
#[must_use]
pub fn ui_layout_indent(depth: u16) -> f32 {
    f32::from(depth) * OutlinerOptions::default().indent
}

/// How many steps of [`UI_LAYOUT_STRIPE_STEP`] the item `id` draws.
#[must_use]
pub const fn ui_layout_stripe_steps(id: u64) -> u64 {
    id % UI_LAYOUT_STRIPE_CYCLE + 1
}

/// Where every part of the scene was laid out in its last frame, in screen
/// pixels, each as the `(min, max)` of its border box.
#[derive(Clone, Debug, PartialEq)]
pub struct UiLayoutLayout {
    /// The outliner.
    pub outliner: (Vec2, Vec2),
    /// How far it is scrolled, in pixels.
    pub outliner_offset: f32,
    /// The row the script left selected.
    pub selected: (Vec2, Vec2),
    /// The dock pane holding the outliner.
    pub outline_pane: (Vec2, Vec2),
    /// The dock pane holding the tabs.
    pub views_pane: (Vec2, Vec2),
    /// The dock pane under it.
    pub log_pane: (Vec2, Vec2),
    /// Each tab of the strip, in [`UI_LAYOUT_TABS`]' order.
    pub tabs: Vec<(Vec2, Vec2)>,
    /// The showing tab's pane.
    pub tab_pane: (Vec2, Vec2),
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
    /// The dock's outer divider.
    Divider,
    /// The row the click focuses and selects.
    Row,
    /// The tab the script shows.
    Tab,
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

/// How many steps down the pad takes, from the row the click focused.
const WALK: usize = UI_LAYOUT_SELECTED_ROW - UI_LAYOUT_CLICKED_ROW;

/// The script's length.
const SCRIPT_FRAMES: usize = 10 + WALK;

/// The scripted input, one frame each after the frame that first lays the page
/// out; see the module docs. A drag's later frames point at where the drag
/// began — the divider moves under the pointer — so each names the same part's
/// centre as its first frame, plus the distance dragged. Every frame the array
/// does not name is a bare `NAVIGATION`: the pad speaking and moving nothing.
const SCRIPT: [Input; SCRIPT_FRAMES] = {
    use crate::ui::tree::Direction;
    const DOWN: Input = Input::Nav(NavInput::toward(Direction::Down));
    let mut script = [Input::Nav(NavInput::NAVIGATION); SCRIPT_FRAMES];
    // The pad speaks, focus lands on the outliner's first row, and right opens
    // it — the WAI-ARIA tree view rule, on the frame.
    script[1] = Input::Nav(NavInput::toward(Direction::Right));
    script[2] = press(Part::Divider, 0.0, 0.0);
    script[3] = press(Part::Divider, UI_LAYOUT_DIVIDER_DRAG, 0.0);
    script[4] = release(Part::Divider, UI_LAYOUT_DIVIDER_DRAG, 0.0);
    script[5] = press(Part::Row, 0.0, 0.0);
    script[6] = release(Part::Row, 0.0, 0.0);
    let mut step = 0;
    while step < WALK {
        script[7 + step] = DOWN;
        step += 1;
    }
    script[7 + WALK] = Input::Nav(NavInput::ACCEPT);
    script[8 + WALK] = press(Part::Tab, 0.0, 0.0);
    script[9 + WALK] = release(Part::Tab, 0.0, 0.0);
    script
};

/// The keys the claims and the script need, from one frame.
#[derive(Clone, Debug)]
struct Parts {
    outliner: NodeKey,
    /// The row the script's click focuses and selects, and its toggle.
    row: NodeKey,
    /// The row the pad walks focus to, when that row is built.
    selected: Option<NodeKey>,
    /// The dock's outer divider.
    divider: NodeKey,
    outline_pane: NodeKey,
    views_pane: NodeKey,
    log_pane: NodeKey,
    /// Each tab of the strip, in [`UI_LAYOUT_TABS`]' order.
    tabs: Vec<NodeKey>,
    /// The showing tab's pane.
    tab_pane: NodeKey,
}

impl Parts {
    /// The key the script aims `part` at.
    fn of(&self, part: Part) -> NodeKey {
        match part {
            Part::Divider => self.divider,
            Part::Row => self.row,
            Part::Tab => self.tabs[UI_LAYOUT_SHOWN_TAB],
        }
    }
}

/// What the scene's widgets keep between frames.
struct State {
    outliner: OutlinerState,
    dock: DockLayout,
}

impl State {
    fn new() -> Self {
        // The nested branch starts open, so the model has a third level as
        // soon as the root is opened; see the module docs.
        let mut outliner = OutlinerState::new();
        outliner.set_expanded(OutlinerId(UI_LAYOUT_NESTED), true);
        Self {
            outliner,
            dock: DockLayout::split(
                SplitAxis::Row,
                DockLayout::pane("outline"),
                DockLayout::split(
                    SplitAxis::Column,
                    DockLayout::pane("views"),
                    DockLayout::pane("log"),
                ),
            ),
        }
    }
}

/// What one frame's builders recorded: the key of each node a widget called
/// back into, by a name the frame can look up.
type Found = Vec<(String, NodeKey)>;

/// The dock's panes. Each records the `.dock-pane` it is filling, which is the
/// block whose builder is running.
fn pane(ui: &mut Ui, name: &str, outliner: &mut OutlinerState, found: &mut Found) {
    found.push((
        format!("pane:{name}"),
        ui.current_key().expect("inside a pane"),
    ));
    match name {
        "outline" => {
            let options = OutlinerOptions {
                row_height: UI_LAYOUT_ROW_HEIGHT,
                select: SelectMode::Replace,
                ..OutlinerOptions::default()
            };
            let mut rows = Found::new();
            let key = ui
                .outliner_with("#tree", outliner, &options, tree, |ui, row| {
                    rows.push((
                        format!("row:{}", row.id.0),
                        ui.current_key().expect("inside a row"),
                    ));
                    let stripe = if row.id.0.is_multiple_of(2) {
                        ".stripe.even"
                    } else {
                        ".stripe"
                    };
                    let width = ui_layout_stripe_steps(row.id.0) as f32 * UI_LAYOUT_STRIPE_STEP;
                    ui.block(stripe, &[Declaration::Width(LengthAuto::Px(width))], |_| {});
                })
                .key;
            found.push(("outliner".to_owned(), key));
            found.append(&mut rows);
        }
        "views" => {
            let mut fill = None;
            let key = ui
                .tabs("#views", &UI_LAYOUT_TABS, |ui, index| {
                    fill = ui.current_key();
                    ui.block(&format!(".tab-fill.fill-{index}"), &[], |_| {});
                })
                .key;
            found.push(("tabs".to_owned(), key));
            found.push((
                "tab-pane".to_owned(),
                fill.expect("the showing tab's pane is built"),
            ));
        }
        _ => {
            ui.block("#log-fill", &[], |_| {});
        }
    }
}

fn frame(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    state: &mut State,
    extent: (u32, u32),
) -> Parts {
    ui.begin_frame_with(pointer, nav);
    let size = [
        Declaration::Width(LengthAuto::Px(extent.0 as f32)),
        Declaration::Height(LengthAuto::Px(extent.1 as f32)),
    ];
    let mut found = Found::new();
    let mut dock = None;
    let outliner = &mut state.outliner;
    let layout = &mut state.dock;
    ui.block(".page", &size, |ui| {
        dock = Some(
            ui.dock("#docks", layout, UI_LAYOUT_PANE_MIN, |ui, name| {
                pane(ui, name, outliner, &mut found);
            })
            .key,
        );
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::new(extent.0 as f32, extent.1 as f32)),
        &FontAtlas::built_in(),
    );
    parts(
        ui,
        dock.expect("the dock is built"),
        &found,
        &state.outliner,
    )
}

/// The keys the script and the claims need, read off the frame just built.
fn parts(ui: &Ui, dock: NodeKey, found: &Found, state: &OutlinerState) -> Parts {
    let named = |wanted: &str| {
        found
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, key)| *key)
            .unwrap_or_else(|| panic!("{wanted} was not built"))
    };
    let row_of = |index: usize| {
        let id = state.rows().get(index)?.id.0;
        let wanted = format!("row:{id}");
        found
            .iter()
            .find(|(name, _)| name == &wanted)
            .map(|(_, key)| *key)
    };
    // The dock's one child is its root split, whose children are the first
    // pane, the divider and the second pane.
    let root = ui.child_keys(dock);
    let divider = ui
        .child_keys(*root.first().expect("the dock holds its layout"))
        .get(1)
        .copied()
        .expect("a split holds a divider between its panes");
    let tabs = named("tabs");
    let strip = *ui
        .child_keys(tabs)
        .first()
        .expect("the tabs hold their strip");
    Parts {
        outliner: named("outliner"),
        row: row_of(UI_LAYOUT_CLICKED_ROW).unwrap_or_else(|| named("outliner")),
        selected: row_of(UI_LAYOUT_SELECTED_ROW),
        divider,
        outline_pane: named("pane:outline"),
        views_pane: named("pane:views"),
        log_pane: named("pane:log"),
        tabs: ui.child_keys(strip),
        tab_pane: named("tab-pane"),
    }
}

/// Runs the script and returns the tree after its last frame with that frame's
/// keys and the state the widgets were left with.
fn build(extent: (u32, u32)) -> (Ui, Parts, State) {
    let mut ui = Ui::new();
    ui.add_stylesheet("ui_layout.css", &ui_layout_css());
    let mut state = State::new();
    let away = PointerInput::hovering(Vec2::splat(-1.0));
    let mut parts = frame(&mut ui, away, NavInput::default(), &mut state, extent);
    // A drag is aimed at where its part was when the drag began.
    let mut anchor = None;
    for input in SCRIPT {
        let (pointer, nav) = match input {
            Input::Pointer { part, offset, down } => {
                let (min, max) = ui.rect(parts.of(part)).expect("laid out");
                let centre = *anchor.get_or_insert((min + max) * 0.5);
                if !down {
                    anchor = None;
                }
                let pointer = PointerInput {
                    pos: centre + offset,
                    down,
                    released: !down,
                };
                (pointer, NavInput::default())
            }
            Input::Nav(nav) => (away, nav),
        };
        parts = frame(&mut ui, pointer, nav, &mut state, extent);
    }
    (ui, parts, state)
}

/// The layout of [`Scene::UiLayout`](super::Scene::UiLayout) in an
/// `extent`-sized frame, as its last frame laid it out.
#[must_use]
pub fn ui_layout_layout(extent: (u32, u32)) -> UiLayoutLayout {
    let (ui, parts, _) = build(extent);
    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    UiLayoutLayout {
        outliner: rect(parts.outliner),
        outliner_offset: ui.scroll_offset_of(parts.outliner).y,
        selected: rect(parts.selected.expect("the selected row is built")),
        outline_pane: rect(parts.outline_pane),
        views_pane: rect(parts.views_pane),
        log_pane: rect(parts.log_pane),
        tabs: parts.tabs.iter().map(|&key| rect(key)).collect(),
        tab_pane: rect(parts.tab_pane),
    }
}

/// The scene's draw list for an `extent`-sized frame: the script's last frame.
#[must_use]
pub fn ui_layout_draw_list(extent: (u32, u32)) -> DrawList {
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

    /// `default.css`'s divider thickness, for both axes.
    const DIVIDER: f32 = 4.0;

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
        ui_layout_draw_list(EXTENT)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, color } if is(*color, srgb) => Some((*min, *max)),
                _ => None,
            })
            .collect()
    }

    /// The scene is what its claims need before any pixel is read: the root
    /// expanded, the outliner scrolled so the selected row is the row the pad
    /// walked to, exactly one row selected, the middle tab showing, the dock's
    /// outer divider away from its even share by what was dragged, and the
    /// stylesheet parsed cleanly.
    #[test]
    fn the_script_leaves_every_surface_where_the_claims_need_it() {
        let logs = crcbl_core::log::capture();
        let (ui, parts, state) = build(EXTENT);
        assert!(
            logs.records().is_empty(),
            "the scene reported {:#?}",
            logs.records()
        );

        let rows = ui_layout_rows();
        assert_eq!(
            state
                .outliner
                .rows()
                .iter()
                .map(|row| (row.id.0, row.depth))
                .collect::<Vec<_>>(),
            rows,
            "the model the claims read is not the model the scene showed"
        );
        assert!(
            rows.len() > UI_LAYOUT_SELECTED_ROW * 2,
            "the model is too short for the walk to scroll it"
        );
        assert!(
            state.outliner.is_expanded(OutlinerId(0)),
            "right did not expand the root, so the model is one row"
        );

        let selected: Vec<u64> = state.outliner.selected().map(|id| id.0).collect();
        assert_eq!(
            selected,
            [rows[UI_LAYOUT_SELECTED_ROW].0],
            "not exactly the walked-to row selected"
        );
        assert_eq!(
            ui.focused(),
            Some(parts.tabs[UI_LAYOUT_SHOWN_TAB]),
            "the last click did not leave focus on the tab it showed"
        );

        let layout = ui_layout_layout(EXTENT);
        let view = layout.outliner.1.y - layout.outliner.0.y;
        assert!(
            layout.outliner_offset > view,
            "the outliner scrolled by {} px, less than its own view",
            layout.outliner_offset
        );
        let depths: Vec<u16> = rows
            .iter()
            .skip((layout.outliner_offset / UI_LAYOUT_ROW_HEIGHT) as usize)
            .take((view / UI_LAYOUT_ROW_HEIGHT) as usize)
            .map(|&(_, depth)| depth)
            .collect();
        assert!(
            depths.iter().min() != depths.iter().max(),
            "every row in view is at one depth, so the picture shows no nesting"
        );
        let (min, max) = layout.selected;
        assert!(
            min.y >= layout.outliner.0.y && max.y <= layout.outliner.1.y,
            "the selected row is outside the view"
        );

        // The dock: the drag moved the outer divider off the even share, and
        // the two panes still meet one divider apart.
        let room = EXTENT.0 as f32 - 2.0 * UI_LAYOUT_PADDING;
        let even = (room - DIVIDER) / 2.0;
        let outline = layout.outline_pane.1.x - layout.outline_pane.0.x;
        assert_eq!(
            outline,
            even + UI_LAYOUT_DIVIDER_DRAG,
            "the outer divider is not where the drag left it"
        );
        assert_eq!(
            layout.views_pane.0.x - layout.outline_pane.1.x,
            DIVIDER,
            "the panes do not meet one divider apart"
        );
        assert_eq!(
            layout.log_pane.0.y - layout.views_pane.1.y,
            DIVIDER,
            "the nested panes do not meet one divider apart"
        );

        // The tabs fit the pane they sit in, so none is clipped away.
        let strip = layout.tabs.last().expect("a tab").1.x;
        assert!(
            strip <= layout.views_pane.1.x,
            "the tab strip at {strip} overflows its pane at {}",
            layout.views_pane.1.x
        );
    }

    /// **The constants the frame is measured against are the colours the frame
    /// draws**: exactly one row in the selection fill, exactly one tab in the
    /// accent, the showing tab's pane fill drawn once and the other two not at
    /// all, and the bottom dock pane's fill drawn once.
    #[test]
    fn the_scenes_colours_are_the_ones_drawn() {
        let layout = ui_layout_layout(EXTENT);
        assert_eq!(
            rects_of(UI_LAYOUT_SELECTED),
            [layout.selected],
            "not exactly the selected row filled"
        );
        assert_eq!(
            rects_of(UI_LAYOUT_ACCENT),
            [layout.tabs[UI_LAYOUT_SHOWN_TAB]],
            "not exactly the showing tab in the accent"
        );
        for (index, fill) in UI_LAYOUT_TAB_FILLS.iter().enumerate() {
            let drawn = rects_of(*fill);
            if index == UI_LAYOUT_SHOWN_TAB {
                assert_eq!(drawn.len(), 1, "the showing tab's pane is not drawn once");
            } else {
                assert!(
                    drawn.is_empty(),
                    "tab {index}'s pane is drawn although it is not showing"
                );
            }
        }
        assert_eq!(
            rects_of(UI_LAYOUT_LOG_FILL).len(),
            1,
            "the bottom dock pane is not drawn once"
        );

        // Every stripe the view shows is one of the two stripe colours, and no
        // two rows in view draw the same width.
        let widths: Vec<f32> = rects_of(UI_LAYOUT_STRIPE_EVEN)
            .into_iter()
            .chain(rects_of(UI_LAYOUT_STRIPE_ODD))
            .map(|(min, max)| max.x - min.x)
            .collect();
        assert!(widths.len() > 4, "the view shows {} stripes", widths.len());
        assert!(
            widths
                .iter()
                .all(|&width| width <= UI_LAYOUT_STRIPE_CYCLE as f32 * UI_LAYOUT_STRIPE_STEP),
            "a stripe is wider than the widest item draws: {widths:?}"
        );
    }
}
