//! [`Scene::UiInspector`](super::Scene::UiInspector)'s content: the
//! reflection-driven property inspector of `docs/plan/07-ui-debug.md` rung 8,
//! over a component with one field of every shape a row is drawn for, styled by
//! the engine's `default.css` and driven by a scripted pointer and pad so that
//! what the panel promises can be read back off the frame.
//!
//! A scene of its own rather than more of `ui_layout`: that page is already two
//! dock panes of outliner and tabs at this extent, and every one of its claims
//! is a measured rectangle that a third surface would move. The inspector also
//! wants a column tall enough to hold an open group beside a shut one, which is
//! the whole of what "a nested struct's rows appear only while its header is
//! open" is read from.
//!
//! # The component, and why it is a copy
//!
//! [`UiInspectorSurface`] is field for field `apps/puppet`'s `Surface` — a
//! `String`, a `[f64; 3]` position, a nested enum whose rows change with the
//! variant, and a ranged `[f32; 3]` tint — with a nested struct, a ranged
//! stepped `f64` and a flag added so that every arm of the widget has a row
//! here. **It is a copy because it has to be**: this crate is the engine and
//! `apps/puppet` depends on it, so the arrow cannot point the other way. The
//! real component is held to the same rows by `apps/puppet`'s own
//! `an_inspector_over_a_surface_draws_its_rows_and_reports_an_undoable_edit`.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ┌─ .page ─────────────────────────────────┐
//!   │ ┌─ inspector #props ──────────────────┐ │
//!   │ │ Label     [floor              ]     │ │
//!   │ │ Position  x[1.00] y[2.00] z[3.00]   │ │ ← the vector override
//!   │ │ - UiInspectorShape: Dome                       │ │ ← the script opened it
//!   │ │     Radius   [6.0]                  │ │ ← the only nested row
//!   │ │ + UiInspectorMotion                            │ │ ← still shut
//!   │ │ Tint      x[0.50] y[0.25] z[0.13]   │ │ ← the override on `[f32; 3]`
//!   │ │ Height    [4.0]                     │ │ ← engaged, stepped to its max
//!   │ │ Visible   [x]                       │ │
//!   │ └─────────────────────────────────────┘ │
//!   │ ▓▓▓▓▓▓▓▓░░░░░░░░  the gauge             │
//!   └─────────────────────────────────────────┘
//! ```
//!
//! The script, one frame each after the frame that first lays the page out: a
//! click on the shape group's header opens it; a click on the height row's
//! drag-value engages it — a press that does not move, which is the pointer
//! half of the LOCKED rule; then [`UI_INSPECTOR_STEPS`] steps right, which is
//! more than the field's range holds.
//!
//! * **Each top-level row** draws in [`UI_INSPECTOR_ROW`] and **each row inside
//!   an open group** in [`UI_INSPECTOR_NESTED`], so the frame says how many rows
//!   are at each depth. A shut group that built its body would put three more
//!   nested bands on the picture.
//! * **The engaged drag-value** is `default.css`'s
//!   [`UI_INSPECTOR_ENGAGED`], and exactly one widget on the frame is in it.
//! * **The gauge** is the scene's own readout of what the inspector left in the
//!   component: its fill is the field's value as a share of
//!   [`UI_INSPECTOR_GAUGE_SPAN`], which is twice the field's maximum — so a
//!   drag that ignored `Field::range` would run off the end of the track and a
//!   drag that ignored `Field::step` would stop short of half of it.
//!
//! Every colour and length the frame is measured against is a constant here.

use glam::Vec2;

use crcbl_reflect::Reflect;

use crate::ui::draw_list::DrawList;
use crate::ui::style::Declaration;
use crate::ui::text::FontAtlas;
use crate::ui::tree::{
    AvailableSpace, Direction, FieldEdit, FlexDirection, InspectorOptions, LengthAuto, NavInput,
    NodeKey, Overrides, Ui,
};
use crate::ui::widget::PointerInput;

/// The page's fill, as the stylesheet writes it: sRGB bytes.
pub const UI_INSPECTOR_PAGE: [u8; 3] = [0x10, 0x12, 0x16];

/// The fill a row at the top of the panel draws.
pub const UI_INSPECTOR_ROW: [u8; 3] = [0x28, 0x36, 0x48];

/// The fill a row inside an open group draws.
pub const UI_INSPECTOR_NESTED: [u8; 3] = [0x60, 0x38, 0x70];

/// `default.css`'s engaged drag-value fill.
pub const UI_INSPECTOR_ENGAGED: [u8; 3] = [0x1d, 0x35, 0x57];

/// The gauge track's fill.
pub const UI_INSPECTOR_GAUGE_TRACK: [u8; 3] = [0x3a, 0x30, 0x20];

/// The gauge fill's own.
pub const UI_INSPECTOR_GAUGE_FILL: [u8; 3] = [0xd0, 0x90, 0x20];

/// How wide the gauge's track is, in pixels.
pub const UI_INSPECTOR_GAUGE_WIDTH: f32 = 64.0;

/// How tall it is.
pub const UI_INSPECTOR_GAUGE_HEIGHT: f32 = 6.0;

/// The value the gauge's whole width stands for: twice
/// [`UI_INSPECTOR_HEIGHT_MAX`], so a field held inside its range fills exactly
/// half the track and one that ran past it runs off the end.
pub const UI_INSPECTOR_GAUGE_SPAN: f64 = 2.0 * UI_INSPECTOR_HEIGHT_MAX;

/// The height field's maximum, as `UiInspectorSurface` declares it.
pub const UI_INSPECTOR_HEIGHT_MAX: f64 = 4.0;

/// The step one notch moves it by, as `UiInspectorSurface` declares it.
pub const UI_INSPECTOR_HEIGHT_STEP: f64 = 0.5;

/// What the height field starts at.
pub const UI_INSPECTOR_HEIGHT_START: f64 = 1.0;

/// How many steps right the script takes on the engaged drag-value: more than
/// the range holds, so the last of them changes nothing and the field is left
/// at its maximum.
pub const UI_INSPECTOR_STEPS: usize = 12;

/// How many rows sit at the top of the panel: the label, the two rows the
/// vector override draws, the height and the flag — the shape and the motion
/// are groups instead.
pub const UI_INSPECTOR_TOP_ROWS: usize = 5;

/// How many rows the one open group builds: the dome variant's single field.
pub const UI_INSPECTOR_NESTED_ROWS: usize = 1;

/// The page's padding, in pixels.
pub const UI_INSPECTOR_PADDING: f32 = 2.0;

/// The gap between the panel and the gauge under it, in pixels.
pub const UI_INSPECTOR_GAP: f32 = 4.0;

/// The label of the group the script opens, as the header shows it — the
/// active variant after the field's own label.
pub const UI_INSPECTOR_OPEN_GROUP: &str = "Shape: Dome";

/// The label of the group the script leaves shut.
pub const UI_INSPECTOR_SHUT_GROUP: &str = "Motion";

/// How many rows the shut group would build if it built its body — the number
/// the nested claim would read instead of [`UI_INSPECTOR_NESTED_ROWS`].
pub const UI_INSPECTOR_SHUT_ROWS: usize = 2;

// ---------------------------------------------------------------------------
// The component
// ---------------------------------------------------------------------------

/// The component the panel is built over; see the module docs.
#[derive(Clone, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl_reflect")]
pub struct UiInspectorSurface {
    /// A text leaf.
    #[reflect(name = "Label")]
    pub label: String,
    /// A list the vector override draws as one row.
    #[reflect(name = "Position")]
    pub position: [f64; 3],
    /// The nested enum the script opens.
    #[reflect(name = "Shape")]
    pub shape: UiInspectorShape,
    /// The nested struct the script leaves shut, so the frame carries a group
    /// whose body is not built beside one whose body is.
    #[reflect(name = "Motion")]
    pub motion: UiInspectorMotion,
    /// A second list the vector override covers, and the one that carries a
    /// range: three channels of linear RGB.
    #[reflect(name = "Tint", min = 0.0, max = 1.0, step = 0.01)]
    pub tint: [f32; 3],
    /// The ranged, stepped leaf the script engages and steps past its end.
    #[reflect(name = "Height", min = 0.0, max = 4.0, step = 0.5)]
    pub height: f64,
    /// A flag, so a checkbox is on the frame.
    #[reflect(name = "Visible")]
    pub visible: bool,
    /// Derived from the rest, so no row may offer it.
    #[reflect(skip)]
    pub cached_area: f64,
}

/// The nested struct the script leaves shut: two rows a shut header does not
/// build.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl_reflect")]
pub struct UiInspectorMotion {
    /// How fast it moves.
    #[reflect(name = "Speed", min = 0.0, max = 10.0, step = 0.5)]
    pub speed: f64,
    /// Whether it repeats.
    #[reflect(name = "Looping")]
    pub looping: bool,
}

/// The nested enum: `Reflect` describes the active variant's fields alone.
#[derive(Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(crate = "crcbl_reflect")]
pub enum UiInspectorShape {
    /// The variant the scene is not in.
    Platform {
        /// Its extent along `X`.
        #[reflect(name = "Width", min = 0.0, max = 64.0, step = 0.1)]
        width: f64,
        /// Its extent along `Z`.
        #[reflect(name = "Depth", min = 0.0, max = 64.0, step = 0.1)]
        depth: f64,
    },
    /// The variant it is in: one field, so one nested row.
    Dome {
        /// Its radius.
        #[reflect(name = "Radius", min = 0.0, max = 64.0, step = 0.1)]
        radius: f64,
    },
}

/// The component as the scene's first frame finds it.
#[must_use]
pub fn ui_inspector_surface() -> UiInspectorSurface {
    UiInspectorSurface {
        label: "floor".to_owned(),
        position: [1.0, 2.0, 3.0],
        shape: UiInspectorShape::Dome { radius: 6.0 },
        motion: UiInspectorMotion {
            speed: 1.0,
            looping: false,
        },
        tint: [0.5, 0.25, 0.125],
        height: UI_INSPECTOR_HEIGHT_START,
        visible: true,
        cached_area: 99.0,
    }
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The scene's stylesheet: the page's layout, the two row fills the depth claim
/// is read from, and the gauge.
///
/// Every colour a widget draws is `default.css`'s; what is restated here is
/// **padding and gaps only**, because nine rows of the engine's own spacing are
/// taller than the 192-pixel frame every UI golden is blessed at, and three
/// drag-values with their own padding on one vector row are wider than it.
/// Squeezing the page is the scene's business, as `ui_layout`'s own row height
/// is.
#[must_use]
pub fn ui_inspector_css() -> String {
    format!(
        "\
.page {{
  flex-direction: column;
  gap: {gap}px;
  padding: {padding}px;
  background: {page};
}}
inspector {{
  gap: 1px;
  padding: 0;
}}
inspector > .inspector-row {{
  background: {row};
}}
.inspector-row {{
  gap: 2px;
  padding: 0 2px;
}}
.collapsing-header {{
  padding: 0 6px;
}}
.collapsing-body {{
  padding: 1px 6px;
  gap: 1px;
}}
.inspector-axis {{
  gap: 1px;
}}
.inspector-axis > .inspector-field {{
  padding: 0 1px;
}}
.collapsing-body .inspector-row {{
  background: {nested};
}}
#gauge {{
  width: {width}px;
  height: {height}px;
  flex-shrink: 0;
  background: {track};
}}
.gauge-fill {{
  align-self: stretch;
  flex-shrink: 0;
  background: {fill};
}}
",
        gap = UI_INSPECTOR_GAP,
        padding = UI_INSPECTOR_PADDING,
        page = hex(UI_INSPECTOR_PAGE),
        row = hex(UI_INSPECTOR_ROW),
        nested = hex(UI_INSPECTOR_NESTED),
        width = UI_INSPECTOR_GAUGE_WIDTH,
        height = UI_INSPECTOR_GAUGE_HEIGHT,
        track = hex(UI_INSPECTOR_GAUGE_TRACK),
        fill = hex(UI_INSPECTOR_GAUGE_FILL),
    )
}

/// How wide the gauge's fill is for `height`, in pixels: the value as a share
/// of [`UI_INSPECTOR_GAUGE_SPAN`], and **not clamped** — a field that ran past
/// its range runs past the track.
#[must_use]
pub fn ui_inspector_gauge(height: f64) -> f32 {
    (height / UI_INSPECTOR_GAUGE_SPAN) as f32 * UI_INSPECTOR_GAUGE_WIDTH
}

// ---------------------------------------------------------------------------
// The script
// ---------------------------------------------------------------------------

/// The parts the script points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    /// The shape group's header row.
    Group,
    /// The height row's drag-value.
    Height,
}

/// One scripted frame.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Input {
    /// The pointer on `part`, held or released.
    Pointer { part: Part, down: bool },
    /// A navigation input, the pointer away from everything.
    Nav(NavInput),
}

/// The script's length: the two clicks, then the steps.
const SCRIPT_FRAMES: usize = 4 + UI_INSPECTOR_STEPS;

/// The scripted input, one frame each after the frame that first lays the page
/// out; see the module docs.
const SCRIPT: [Input; SCRIPT_FRAMES] = {
    const RIGHT: Input = Input::Nav(NavInput::toward(Direction::Right));
    let mut script = [RIGHT; SCRIPT_FRAMES];
    script[0] = Input::Pointer {
        part: Part::Group,
        down: true,
    };
    script[1] = Input::Pointer {
        part: Part::Group,
        down: false,
    };
    script[2] = Input::Pointer {
        part: Part::Height,
        down: true,
    };
    script[3] = Input::Pointer {
        part: Part::Height,
        down: false,
    };
    script
};

/// The keys the claims and the script need, from one frame.
#[derive(Clone, Debug)]
struct Parts {
    /// The `inspector` block.
    panel: NodeKey,
    /// Each row at the top of the panel, in build order.
    rows: Vec<NodeKey>,
    /// Each row inside an open group, in build order.
    nested: Vec<NodeKey>,
    /// The group the script opens: its header row.
    group: NodeKey,
    /// The group it leaves shut: its header row.
    shut: NodeKey,
    /// The height row's drag-value.
    height: NodeKey,
    /// The gauge's fill.
    gauge: NodeKey,
}

impl Parts {
    /// The key the script aims `part` at.
    const fn of(&self, part: Part) -> NodeKey {
        match part {
            Part::Group => self.group,
            Part::Height => self.height,
        }
    }
}

/// One frame: build the page over `surface`, lay it out, and return this
/// frame's keys with the edits the panel reported.
fn frame(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    surface: &mut UiInspectorSurface,
    overrides: &Overrides,
    extent: (u32, u32),
) -> (Parts, Vec<FieldEdit>) {
    ui.begin_frame_with(pointer, nav);
    let size = [
        Declaration::FlexDirection(FlexDirection::Column),
        Declaration::Width(LengthAuto::Px(extent.0 as f32)),
        Declaration::Height(LengthAuto::Px(extent.1 as f32)),
    ];
    let mut panel = None;
    let mut gauge = None;
    ui.block(".page", &size, |ui| {
        let options = InspectorOptions {
            overrides: Some(overrides),
            ..InspectorOptions::default()
        };
        panel = Some(ui.inspector_with("#props", surface, &options));
        // The scene's own readout of what the panel left in the component.
        let width = ui_inspector_gauge(surface.height);
        ui.block("#gauge", &[], |ui| {
            gauge = Some(
                ui.block(
                    ".gauge-fill",
                    &[Declaration::Width(LengthAuto::Px(width))],
                    |_| {},
                )
                .key,
            );
        });
    });
    ui.layout(
        Vec2::ZERO,
        AvailableSpace::definite(Vec2::new(extent.0 as f32, extent.1 as f32)),
        &FontAtlas::built_in(),
    );

    let panel = panel.expect("the page builds the inspector");
    let children = ui.child_keys(panel.response.key);
    assert_eq!(
        children.len(),
        UI_INSPECTOR_TOP_ROWS + 2,
        "the panel is not one row per field of the component"
    );
    // The component's fields in order: label, position, shape, motion, tint,
    // height, visible — so the two groups are the third and the fourth and the
    // rest are rows, the two lists among them through the vector override. A
    // group's first child is its header; its second, while open, is its body.
    let group_parts = |ui: &Ui, key: NodeKey| {
        let parts = ui.child_keys(key);
        let header = *parts.first().expect("a collapsing holds its header");
        let body = parts.get(1).map(|&body| ui.child_keys(body));
        (header, body)
    };
    let (group, body) = group_parts(ui, children[2]);
    let (shut, shut_body) = group_parts(ui, children[3]);
    assert!(
        shut_body.is_none(),
        "the group the script never opens built a body"
    );
    let rows = vec![
        children[0],
        children[1],
        children[4],
        children[5],
        children[6],
    ];
    let height = ui.child_keys(children[5])[1];

    (
        Parts {
            panel: panel.response.key,
            rows,
            nested: body.unwrap_or_default(),
            group,
            shut,
            height,
            gauge: gauge.expect("the gauge is built"),
        },
        panel.edits,
    )
}

/// Runs the script and returns the tree after its last frame, that frame's
/// keys, the component the panel left behind, and every edit it reported.
fn build(extent: (u32, u32)) -> (Ui, Parts, UiInspectorSurface, Vec<FieldEdit>) {
    let mut ui = Ui::new();
    ui.add_stylesheet("ui_inspector.css", &ui_inspector_css());
    let overrides = Overrides::vectors();
    let mut surface = ui_inspector_surface();
    let away = PointerInput::hovering(Vec2::splat(-1.0));

    let (mut parts, _) = frame(
        &mut ui,
        away,
        NavInput::default(),
        &mut surface,
        &overrides,
        extent,
    );
    let mut edits = Vec::new();
    for input in SCRIPT {
        let (pointer, nav) = match input {
            Input::Pointer { part, down } => {
                let (min, max) = ui.rect(parts.of(part)).expect("laid out");
                let pointer = PointerInput {
                    pos: (min + max) * 0.5,
                    down,
                    released: !down,
                };
                (pointer, NavInput::default())
            }
            Input::Nav(nav) => (away, nav),
        };
        let (next, made) = frame(&mut ui, pointer, nav, &mut surface, &overrides, extent);
        parts = next;
        edits.extend(made);
    }
    (ui, parts, surface, edits)
}

/// Where every part of the scene was laid out in its last frame, in screen
/// pixels, each as the `(min, max)` of its border box.
#[derive(Clone, Debug, PartialEq)]
pub struct UiInspectorLayout {
    /// The `inspector` block.
    pub panel: (Vec2, Vec2),
    /// Each row at the top of the panel, in build order.
    pub rows: Vec<(Vec2, Vec2)>,
    /// Each row inside the one open group.
    pub nested: Vec<(Vec2, Vec2)>,
    /// The open group's header row.
    pub group: (Vec2, Vec2),
    /// The shut group's header row.
    pub shut: (Vec2, Vec2),
    /// The engaged drag-value.
    pub height: (Vec2, Vec2),
    /// The gauge's fill.
    pub gauge: (Vec2, Vec2),
}

/// The layout of [`Scene::UiInspector`](super::Scene::UiInspector) in an
/// `extent`-sized frame, as its last frame laid it out.
#[must_use]
pub fn ui_inspector_layout(extent: (u32, u32)) -> UiInspectorLayout {
    let (ui, parts, _, _) = build(extent);
    let rect = |key| ui.rect(key).expect("every reported part was laid out");
    UiInspectorLayout {
        panel: rect(parts.panel),
        rows: parts.rows.iter().map(|&key| rect(key)).collect(),
        nested: parts.nested.iter().map(|&key| rect(key)).collect(),
        group: rect(parts.group),
        shut: rect(parts.shut),
        height: rect(parts.height),
        gauge: rect(parts.gauge),
    }
}

/// What the script left in the component the panel edited.
#[must_use]
pub fn ui_inspector_edited(extent: (u32, u32)) -> UiInspectorSurface {
    let (_, _, surface, _) = build(extent);
    surface
}

/// Every edit the panel reported over the whole script, in order.
#[must_use]
pub fn ui_inspector_edits(extent: (u32, u32)) -> Vec<FieldEdit> {
    let (_, _, _, edits) = build(extent);
    edits
}

/// The scene's draw list for an `extent`-sized frame: the script's last frame.
#[must_use]
pub fn ui_inspector_draw_list(extent: (u32, u32)) -> DrawList {
    let (ui, _, _, _) = build(extent);
    let mut list = DrawList::new();
    ui.emit(&mut list);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw_list::DrawCommand;
    use crcbl_reflect::{Value, get_path, set_path};

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
        ui_inspector_draw_list(EXTENT)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, color } if is(*color, srgb) => Some((*min, *max)),
                _ => None,
            })
            .collect()
    }

    /// Every line of text on the scene's last frame.
    fn texts() -> Vec<String> {
        ui_inspector_draw_list(EXTENT)
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The scene is what its claims need before any pixel is read: the panel
    /// inside the frame, one group open and one shut, the rows at each depth
    /// the constants name, and the stylesheet parsed cleanly.
    #[test]
    fn the_script_leaves_the_panel_where_the_claims_need_it() {
        let logs = crcbl_core::log::capture();
        let layout = ui_inspector_layout(EXTENT);
        assert!(
            logs.records().is_empty(),
            "the scene reported {:#?}",
            logs.records()
        );

        assert_eq!(layout.rows.len(), UI_INSPECTOR_TOP_ROWS);
        assert_eq!(layout.nested.len(), UI_INSPECTOR_NESTED_ROWS);
        // Both headers are on the frame, and the enum's carries the variant
        // whose fields its body holds.
        for title in [UI_INSPECTOR_OPEN_GROUP, UI_INSPECTOR_SHUT_GROUP] {
            assert!(
                texts().contains(&title.to_owned()),
                "no header reads {title:?}: {:?}",
                texts()
            );
        }
        assert!(
            layout.panel.1.y + UI_INSPECTOR_GAP <= layout.gauge.0.y,
            "the gauge overlaps the panel"
        );
        assert!(
            layout.gauge.1.y <= EXTENT.1 as f32,
            "the page is taller than the frame, so the bottom of it is not on the picture"
        );
        // The open group's body sits between its own header and the shut one,
        // which is what makes "only while its header is open" readable.
        let nested = layout.nested[0];
        assert!(
            nested.0.y >= layout.group.1.y && nested.1.y <= layout.shut.0.y,
            "the nested row is not between the two headers"
        );
        assert!(
            nested.0.x > layout.rows[0].0.x,
            "the nested row is not indented past a top-level one"
        );
    }

    /// **`Field::range` and `Field::step` reached the drag-value**: the steps
    /// stopped at the field's maximum, and the number on the frame is shown
    /// with the decimals the step has — a widget that dropped either would show
    /// a different string and leave a different number.
    #[test]
    fn the_ranged_field_stopped_at_its_maximum_and_is_shown_at_its_steps_decimals() {
        let surface = ui_inspector_edited(EXTENT);
        assert_eq!(
            surface.height, UI_INSPECTOR_HEIGHT_MAX,
            "the steps did not stop at the field's maximum"
        );
        // Twelve steps of 0.5 from 1.0 is 7.0; the range is what stopped it.
        assert!(
            UI_INSPECTOR_HEIGHT_START + UI_INSPECTOR_STEPS as f64 * UI_INSPECTOR_HEIGHT_STEP
                > UI_INSPECTOR_HEIGHT_MAX,
            "the script does not take enough steps to reach the end of the range"
        );
        assert!(
            texts().contains(&"4.0".to_owned()),
            "the height is not drawn at its step's one decimal: {:?}",
            texts()
        );
        assert!(
            !texts().contains(&"7.0".to_owned()),
            "a number past the field's maximum is on the frame"
        );
    }

    /// **The panel reported every step as an edit at the field's path, each
    /// carrying the value it replaced — and `set_path` with that value undoes
    /// it.** The steps past the end of the range report nothing, because they
    /// moved nothing.
    #[test]
    fn every_step_was_reported_as_an_undoable_edit_at_the_fields_path() {
        let edits = ui_inspector_edits(EXTENT);
        let moved = ((UI_INSPECTOR_HEIGHT_MAX - UI_INSPECTOR_HEIGHT_START)
            / UI_INSPECTOR_HEIGHT_STEP) as usize;
        assert_eq!(
            edits.len(),
            moved,
            "the steps past the range's end reported edits: {edits:?}"
        );
        assert!(
            edits.iter().all(|edit| edit.path == "height"),
            "an edit named another field: {edits:?}"
        );
        assert_eq!(
            edits[0].before,
            Value::Float(UI_INSPECTOR_HEIGHT_START),
            "the first edit does not carry what the field held"
        );
        assert_eq!(
            edits[moved - 1].after,
            Value::Float(UI_INSPECTOR_HEIGHT_MAX),
            "the last edit did not land on the field's maximum"
        );

        // Undo: the whole run back, newest first, is `set_path` with the value
        // each edit replaced.
        let mut surface = ui_inspector_edited(EXTENT);
        for edit in edits.iter().rev() {
            set_path(&mut surface, &edit.path, &edit.before).expect("an f64 takes a float");
        }
        assert_eq!(
            get_path(&surface, "height"),
            Ok(Value::Float(UI_INSPECTOR_HEIGHT_START)),
            "undoing every edit did not put the field back"
        );
        assert_eq!(
            surface,
            ui_inspector_surface(),
            "the panel changed something no edit reported"
        );
    }

    /// **The constants the frame is measured against are the colours the frame
    /// draws**: a band per row at each depth, exactly one widget in the engaged
    /// fill, and the gauge's fill at the share the field's maximum names.
    #[test]
    fn the_scenes_colours_are_the_ones_drawn() {
        let layout = ui_inspector_layout(EXTENT);
        assert_eq!(
            rects_of(UI_INSPECTOR_ROW),
            layout.rows,
            "not exactly the top-level rows in the row fill"
        );
        assert_eq!(
            rects_of(UI_INSPECTOR_NESTED),
            layout.nested,
            "not exactly the open group's rows in the nested fill"
        );
        assert_eq!(
            rects_of(UI_INSPECTOR_ENGAGED),
            [layout.height],
            "not exactly the drag-value the script engaged in the engaged fill"
        );

        let gauge = rects_of(UI_INSPECTOR_GAUGE_FILL);
        assert_eq!(gauge.len(), 1, "the gauge's fill is not drawn once");
        let want = ui_inspector_gauge(UI_INSPECTOR_HEIGHT_MAX);
        assert!(
            (gauge[0].1.x - gauge[0].0.x - want).abs() <= 1.0,
            "the gauge is {} px wide, not the {want} the field's maximum names",
            gauge[0].1.x - gauge[0].0.x
        );
        assert!(
            want > 1.0 && want < UI_INSPECTOR_GAUGE_WIDTH - 1.0,
            "the gauge's share is at one end of its track, so nothing distinguishes it"
        );
    }
}
