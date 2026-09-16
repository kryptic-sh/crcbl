//! The property inspector: the row each [`ValueKind`] gets, the range and step
//! reaching the drag-value, recursion under a header, an enum's active
//! variant, a skipped field, an override taking precedence, and the edit a
//! frame reports — with what undoing one takes.

use crcbl_reflect::{Kind, Reflect, Value, get_path, set_path};

use super::*;
use crate::tree::widgets::inspector::{FieldEdit, Inspection, Overrides};
use crate::tree::{Content, NodeKey};

/// A component with one field of every shape a row is drawn for.
///
/// Field for field it is `apps/puppet`'s `Surface` — a `String`, a `[f64; 3]`
/// position, a nested enum whose rows change with the variant, and a ranged
/// `[f32; 3]` tint — with a nested struct, a flag, a count and a skipped field
/// added, so that every arm of the widget has a row here. It is a copy rather
/// than the type itself because `crcbl-ui` sits below `apps/` and cannot name
/// it; `apps/puppet`'s own tests hold the real one to the same rows.
#[derive(Debug, PartialEq, Reflect)]
struct Surface {
    #[reflect(name = "Label")]
    label: String,
    #[reflect(name = "Position")]
    position: [f64; 3],
    #[reflect(name = "Shape")]
    shape: Shape,
    #[reflect(name = "Tint", min = 0.0, max = 1.0, step = 0.01)]
    tint: [f32; 3],
    #[reflect(name = "Motion")]
    motion: Motion,
    /// Ranged and stepped, and a leaf of its own, so one row can be held to
    /// both without opening anything.
    #[reflect(name = "Height", min = 0.0, max = 4.0, step = 0.5)]
    height: f64,
    #[reflect(name = "Visible")]
    visible: bool,
    #[reflect(name = "Layer", min = 0.0, max = 8.0)]
    layer: u32,
    /// Derived from the rest, so a panel must not offer it.
    #[reflect(skip)]
    cached_area: f64,
}

/// The nested struct, whose rows are reached through `Reflect::field_mut`.
#[derive(Debug, PartialEq, Reflect)]
struct Motion {
    #[reflect(name = "Speed", min = 0.0, max = 10.0, step = 0.5)]
    speed: f64,
    #[reflect(name = "Looping")]
    looping: bool,
}

/// The nested enum: only the active variant's fields are described.
#[derive(Debug, PartialEq, Reflect)]
enum Shape {
    Platform {
        #[reflect(name = "Width", min = 0.0, max = 64.0, step = 0.1)]
        width: f64,
        #[reflect(name = "Depth", min = 0.0, max = 64.0, step = 0.1)]
        depth: f64,
    },
    Dome {
        #[reflect(name = "Radius", min = 0.0, max = 64.0, step = 0.1)]
        radius: f64,
    },
}

fn surface() -> Surface {
    Surface {
        label: "floor".to_owned(),
        // More precision than an `f32` holds, so a row that wrote back a field
        // nobody touched would show up as a changed number.
        position: [1.234_567_890_123_4, 2.0, 3.0],
        shape: Shape::Dome { radius: 6.0 },
        tint: [0.5, 0.25, 0.125],
        motion: Motion {
            speed: 1.0,
            looping: false,
        },
        height: 1.000_000_000_000_1,
        visible: true,
        layer: 3,
        cached_area: 99.0,
    }
}

/// One frame of an inspector over `value`, with `overrides` if any.
fn page(
    ui: &mut Ui,
    pointer: PointerInput,
    nav: NavInput,
    value: &mut dyn Reflect,
    overrides: Option<&Overrides>,
) -> Inspection {
    frame(ui, pointer, nav, |ui| {
        let options = InspectorOptions {
            overrides,
            ..InspectorOptions::default()
        };
        ui.inspector_with("#props", value, &options)
    })
}

/// The selector `node` was built from.
fn selector_of(ui: &Ui, index: usize) -> &str {
    let node = &ui.nodes[index];
    &ui.selectors[node.selector.0..node.selector.1]
}

/// The text of every span this frame built whose selector holds `class`, in
/// build order.
fn spans(ui: &Ui, class: &str) -> Vec<String> {
    (0..ui.nodes.len())
        .filter(|&index| selector_of(ui, index).contains(class))
        .filter_map(|index| match ui.nodes[index].content {
            Content::Text { start, end } => Some(ui.text[start..end].to_owned()),
            _ => None,
        })
        .collect()
}

/// Every leaf row this frame built, as `(label, the selector of the widget
/// that edits it)` — the node after the label span is that widget, because a
/// row is built as its label and then its editor.
fn rows(ui: &Ui) -> Vec<(String, String)> {
    (0..ui.nodes.len())
        .filter(|&index| selector_of(ui, index).contains("inspector-label"))
        .filter_map(|index| match ui.nodes[index].content {
            Content::Text { start, end } => {
                let label = ui.text[start..end].to_owned();
                let widget = selector_of(ui, index + 1).to_owned();
                Some((label, widget))
            }
            _ => None,
        })
        .collect()
}

/// The key of the widget editing the row labelled `label`: the node built
/// straight after that row's label span.
fn field_key(ui: &Ui, label: &str) -> NodeKey {
    let index = (0..ui.nodes.len())
        .find(|&index| {
            selector_of(ui, index).contains("inspector-label")
                && matches!(ui.nodes[index].content,
                    Content::Text { start, end } if &ui.text[start..end] == label)
        })
        .unwrap_or_else(|| panic!("no row labelled {label:?} was built"));
    ui.nodes[index + 1].key
}

/// The key of the node this frame built whose collapsing title is `title`: its
/// header row, which is the title span's parent.
fn header(ui: &Ui, title: &str) -> NodeKey {
    let index = (0..ui.nodes.len())
        .find(|&index| {
            selector_of(ui, index).contains("collapsing-title")
                && matches!(ui.nodes[index].content,
                    Content::Text { start, end } if &ui.text[start..end] == title)
        })
        .unwrap_or_else(|| panic!("no header titled {title:?} was built"));
    let parent = ui.nodes[index].parent.expect("a title sits inside its row");
    ui.nodes[parent].key
}

/// Clicks `on` — a press frame and a release frame — and returns the frame
/// after it.
fn click(
    ui: &mut Ui,
    on: Vec2,
    value: &mut dyn Reflect,
    overrides: Option<&Overrides>,
) -> Inspection {
    page(ui, press(on), NavInput::default(), value, overrides);
    page(ui, release(on), NavInput::default(), value, overrides)
}

/// Opens the collapsing header titled `title` and returns the frame that shows
/// its body.
fn open(
    ui: &mut Ui,
    title: &str,
    value: &mut dyn Reflect,
    overrides: Option<&Overrides>,
) -> Inspection {
    let on = centre(ui, header(ui, title));
    click(ui, on, value, overrides)
}

/// **One row per field, and the widget is the one its kind asks for**: a text
/// input for a `String`, a checkbox for a `bool`, a drag-value for a number,
/// and a collapsing header for everything that is not a leaf — and **a field
/// nobody touched is never written**, so an `f64` with more precision than the
/// `f32` the widget shows keeps every digit.
#[test]
fn every_leaf_gets_the_widget_its_kind_asks_for_and_an_untouched_field_is_not_written() {
    let mut ui = Ui::new();
    let mut value = surface();
    let before = surface();
    let inspection = page(&mut ui, idle(), NavInput::default(), &mut value, None);

    assert_eq!(
        rows(&ui),
        [
            ("Label".to_owned(), "text-input.inspector-field".to_owned()),
            ("Height".to_owned(), "drag-value.inspector-field".to_owned()),
            ("Visible".to_owned(), "checkbox.inspector-field".to_owned()),
            ("Layer".to_owned(), "drag-value.inspector-field".to_owned()),
        ],
        "the leaf rows are not the fields' kinds"
    );
    assert_eq!(
        spans(&ui, "collapsing-title"),
        ["Position", "Shape: Dome", "Tint", "Motion"],
        "a composite field did not get a header of its own"
    );
    assert!(
        inspection.edits.is_empty(),
        "a still frame edited something"
    );
    assert_eq!(value, before, "a still frame wrote a field back");
    assert_eq!(
        get_path(&value, "position.0"),
        Ok(Value::Float(1.234_567_890_123_4)),
        "an untouched f64 was narrowed to the f32 its widget shows"
    );
    // **The row that is actually built.** `position` is a shut group, so its
    // leaves are never drawn and the claim above holds vacuously; `height` is a
    // leaf row of its own, drawn every frame, and an `f32` cannot hold it.
    assert_eq!(
        get_path(&value, "height"),
        Ok(Value::Float(1.000_000_000_000_1)),
        "the drawn row narrowed an untouched f64 to the f32 it shows"
    );

    // Whatever else moved, the drag-value's own reading is the field's.
    assert!(
        spans(&ui, "drag-value-text").contains(&"1.0".to_owned()),
        "the height row does not show its field: {:?}",
        spans(&ui, "drag-value-text")
    );
}

/// **`Field::range` and `Field::step` reach the drag-value**: a drag far past
/// the field's maximum stops at it, and the number is shown with the decimals
/// its step has — a row that dropped either would show a different string.
#[test]
fn a_ranged_field_is_dragged_within_its_range_in_its_own_step() {
    let mut ui = Ui::new();
    let mut value = surface();
    page(&mut ui, idle(), NavInput::default(), &mut value, None);

    let on = centre(&ui, field_key(&ui, "Height"));
    page(&mut ui, press(on), NavInput::default(), &mut value, None);
    // One step per pixel, so 200 pixels is 100.0 — far past a maximum of 4.
    let far = on + Vec2::new(200.0, 0.0);
    page(&mut ui, press(far), NavInput::default(), &mut value, None);
    let held = page(&mut ui, release(far), NavInput::default(), &mut value, None);

    assert_eq!(value.height, 4.0, "the drag ran past the field's maximum");
    assert!(
        spans(&ui, "drag-value-text").contains(&"4.0".to_owned()),
        "the height is not shown at its step's one decimal: {:?}",
        spans(&ui, "drag-value-text")
    );
    assert!(
        held.edits.is_empty(),
        "the release frame moved the value again"
    );
}

/// **A nested struct's rows exist only while its header is open**, and the
/// rows are the nested value's own — reached through `Reflect::field_mut`,
/// under a path that names the parent field.
#[test]
fn a_nested_structs_rows_appear_only_under_an_open_header() {
    let mut ui = Ui::new();
    let mut value = surface();
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    let closed: Vec<String> = rows(&ui).into_iter().map(|(label, _)| label).collect();
    assert!(
        !closed.contains(&"Speed".to_owned()) && !closed.contains(&"Looping".to_owned()),
        "a closed header built its body: {closed:?}"
    );

    open(&mut ui, "Motion", &mut value, None);
    let opened: Vec<(String, String)> = rows(&ui);
    assert!(
        opened.contains(&("Speed".to_owned(), "drag-value.inspector-field".to_owned()))
            && opened.contains(&("Looping".to_owned(), "checkbox.inspector-field".to_owned())),
        "the open header did not build the nested value's rows: {opened:?}"
    );
}

/// **An enum shows its active variant's fields and no others**, and its header
/// names the variant those rows belong to.
#[test]
fn an_enum_shows_only_the_active_variants_fields() {
    let mut ui = Ui::new();
    let mut value = surface();
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    assert_eq!(value.shape.kind(), Kind::Enum);

    open(&mut ui, "Shape: Dome", &mut value, None);
    let labels: Vec<String> = rows(&ui).into_iter().map(|(label, _)| label).collect();
    assert!(
        labels.contains(&"Radius".to_owned()),
        "the active variant's field has no row: {labels:?}"
    );
    for other in ["Width", "Depth"] {
        assert!(
            !labels.contains(&other.to_owned()),
            "{other} is the other variant's field and it has a row"
        );
    }

    // The other variant, so the claim is about the *active* one rather than
    // about which fields this enum happens to list first.
    let mut ui = Ui::new();
    let mut value = surface();
    value.shape = Shape::Platform {
        width: 4.0,
        depth: 2.0,
    };
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    open(&mut ui, "Shape: Platform", &mut value, None);
    let labels: Vec<String> = rows(&ui).into_iter().map(|(label, _)| label).collect();
    assert!(
        labels.contains(&"Width".to_owned()) && labels.contains(&"Depth".to_owned()),
        "the platform's fields have no rows: {labels:?}"
    );
    assert!(
        !labels.contains(&"Radius".to_owned()),
        "the dome's field has a row under a platform: {labels:?}"
    );
}

/// **A `#[reflect(skip)]` field has no row anywhere**, at any depth, open or
/// closed.
#[test]
fn a_skipped_field_has_no_row() {
    let mut ui = Ui::new();
    let mut value = surface();
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    for title in ["Position", "Shape: Dome", "Tint", "Motion"] {
        open(&mut ui, title, &mut value, None);
    }
    let labels: Vec<String> = rows(&ui).into_iter().map(|(label, _)| label).collect();
    assert!(
        labels.len() > 8,
        "the page is too small to prove anything: {labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.contains("cached")),
        "the skipped field has a row: {labels:?}"
    );
    assert!(
        !spans(&ui, "collapsing-title")
            .iter()
            .any(|title| title.contains("cached")),
        "the skipped field has a header"
    );
}

/// **An override takes precedence over the value's own kind**: a `[f64; 3]`
/// that would be a collapsing header of three indexed rows is one row of three
/// drag-values instead, and it writes through the same paths — so the same
/// field is a header without the override and a row with it.
#[test]
fn a_registered_override_draws_the_row_instead_of_the_values_own_kind() {
    let mut ui = Ui::new();
    let mut value = surface();
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    assert!(
        spans(&ui, "collapsing-title").contains(&"Position".to_owned()),
        "without an override a list is not a header"
    );

    let overrides = Overrides::vectors();
    let mut ui = Ui::new();
    let mut value = surface();
    page(
        &mut ui,
        idle(),
        NavInput::default(),
        &mut value,
        Some(&overrides),
    );
    assert!(
        !spans(&ui, "collapsing-title").contains(&"Position".to_owned()),
        "the override did not replace the header"
    );
    assert_eq!(
        rows(&ui)
            .into_iter()
            .filter(|(label, _)| label == "Position")
            .collect::<Vec<_>>(),
        [("Position".to_owned(), ".inspector-axis".to_owned())],
        "the vector row is not one row of axes"
    );
    assert_eq!(
        spans(&ui, "inspector-axis-label")
            .into_iter()
            .take(3)
            .collect::<Vec<_>>(),
        ["x", "y", "z"],
        "the vector row is not three labelled components"
    );
}

/// **An edit is reported as the path it was made at and the value it
/// replaced**, and **undoing it is `set_path` with that value** — the whole of
/// what this widget owes the editor's command log.
#[test]
fn an_edit_reports_its_path_and_old_value_and_set_path_undoes_it() {
    let mut ui = Ui::new();
    let mut value = surface();
    page(&mut ui, idle(), NavInput::default(), &mut value, None);
    open(&mut ui, "Motion", &mut value, None);

    let on = centre(&ui, field_key(&ui, "Speed"));
    page(&mut ui, press(on), NavInput::default(), &mut value, None);
    // Five pixels past the drag threshold, at one step (0.5) per pixel.
    let moved = on + Vec2::new(5.0, 0.0);
    let dragged = page(&mut ui, press(moved), NavInput::default(), &mut value, None);

    assert_eq!(
        dragged.edits,
        [FieldEdit {
            path: "motion.speed".to_owned(),
            before: Value::Float(1.0),
            after: Value::Float(3.5),
        }],
        "the drag was not reported as one edit at the nested field's path"
    );
    assert_eq!(value.motion.speed, 3.5);

    // Undo: the same call with the value the edit replaced.
    let edit = &dragged.edits[0];
    set_path(&mut value, &edit.path, &edit.before).expect("the leaf takes its own kind");
    assert_eq!(
        value.motion.speed, 1.0,
        "the undo did not restore the field"
    );
    assert_eq!(
        get_path(&value, &edit.path),
        Ok(edit.before.clone()),
        "the path the edit reported does not read back what it replaced"
    );
}
