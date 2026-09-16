//! The property inspector: `docs/plan/07-ui-debug.md` rung 8's
//! reflection-driven panel with per-type overrides.
//!
//! # What it draws
//!
//! An `inspector` block holding one `.inspector-row` per
//! [`Reflect::fields`] entry — a
//! `.inspector-label` span for the field's
//! [`label`](crcbl_reflect::Field::label), then the widget that edits it,
//! classed `.inspector-field`:
//!
//! | the leaf's [`ValueKind`] | the widget |
//! |---|---|
//! | [`ValueKind::Bool`] | [`Ui::checkbox`] |
//! | [`ValueKind::Int`], [`ValueKind::UInt`], [`ValueKind::Float`] | [`Ui::drag_value`] |
//! | [`ValueKind::Text`] | [`Ui::text_input`] |
//!
//! A field that is not a leaf — a [`Kind::Struct`], a [`Kind::Enum`] or a
//! [`Kind::List`] — is a [`Ui::collapsing`] classed `.inspector-group` whose
//! body holds that value's own rows, reached through
//! [`Reflect::field_mut`]. A struct's and an
//! enum's rows are named by their fields and a list's by their indices, and an
//! enum's header carries its active variant after its label, because
//! [`Reflect::fields`] describes that variant
//! alone. **Depth is styling, not machinery**: the nesting a reader sees is
//! `.collapsing-body`'s padding, and a closed header builds no body at all, so
//! a deep component costs only what is open.
//!
//! # A number's range and step
//!
//! [`Field::range`](crcbl_reflect::Field::range) bounds the drag-value and
//! [`Field::step`](crcbl_reflect::Field::step) is both the step one navigation
//! notch takes and the distance one pixel of a drag moves — one step per pixel,
//! which is the only rule that needs no constant per range. A field with no
//! `#[reflect(step)]` takes [`InspectorOptions::step`] if it is a float and
//! [`WHOLE_STEP`] if it is a whole number, and a field with no
//! `#[reflect(min, max)]` is bounded by the widget's own type rather than by a
//! bound this module invented.
//!
//! **A [`Kind::List`]'s elements inherit the list's own range and step**, and
//! nothing else does. `#[reflect(min = 0.0, max = 1.0)]` on a `[f32; 3]` tint
//! is a statement about each channel — the list itself is not a leaf and has no
//! value of its own to bound — and an element carries no
//! [`Field`](crcbl_reflect::Field) to say otherwise. A [`Kind::Struct`]'s
//! fields do carry their own, so a parent never overwrites them.
//!
//! **The widget edits an `f32` and the leaf may be wider.** [`Ui::drag_value`]
//! takes `&mut f32`; [`Value::Int`] and [`Value::UInt`] are 64 bits wide and
//! [`Value::Float`] is an `f64`. A row therefore narrows only to *show* the
//! value, and writes back **only in the frame the widget reports
//! [`Response::changed`]** — so a field nobody touched is never written and
//! never rounded, and a field that was dragged is written to the precision the
//! person dragged it at. A whole number is rounded to the nearest integer on
//! the way back, so a drag crosses to the next value halfway.
//!
//! # An edit is a command, not a write
//!
//! Every write the panel makes is reported as a [`FieldEdit`]: the dotted
//! [`crcbl_reflect::set_path`] path from the value the inspector was handed,
//! the [`Value`] the field held, and the one it holds now.
//! [`Inspection::edits`] is the frame's list, in the order the rows made them.
//!
//! **Undoing one is the same call with the other value**:
//! `set_path(value, &edit.path, &edit.before)`, against the value the
//! inspector was handed. This module builds no undo stack, no editor-wide
//! command enum and no transport — `docs/plan/08-editor.md` owns those, and the
//! user's decision of 2026-09-16 is that they exist from the editor's first
//! slice. What is here is the half an inspector can honestly produce: the value
//! a caller records.
//!
//! A write the leaf refuses — a number that does not fit, a non-finite float —
//! leaves the field alone and reports nothing, and so does a write that landed
//! on the value already there.
//!
//! # Per-type overrides
//!
//! [`Overrides`] is the caller's, built once and handed to
//! [`Ui::inspector_with`]: [`Overrides::register`] takes a type and a builder,
//! and a value of that type is drawn by the builder instead of by its [`Kind`].
//! This is Unreal's Details-panel customisation and Fyrox's
//! `PropertyEditorDefinition` in the smallest form that works, and it is what
//! [`Reflect::as_any`] exists for — a
//! `&dyn Reflect` alone cannot say which type it is.
//!
//! [`Overrides::vectors`] is the one this crate ships: a three-component vector
//! on one row of three drag-values, which is how every editor of this shape
//! draws a position. It is registered for the four three-component vectors
//! `crcbl-reflect` covers, and a caller registers its own beside it.

use std::any::TypeId;
use std::fmt;

use crcbl_reflect::{Kind, Range, Reflect, Value, ValueKind, get_path, set_path};

use super::{Ui, typed};
use crate::tree::Response;

/// The step a [`ValueKind::Float`] field with no `#[reflect(step)]` is dragged
/// and stepped by: [`InspectorOptions::step`]'s default.
pub const INSPECTOR_STEP: f64 = 0.01;

/// The step a whole number with no `#[reflect(step)]` is dragged and stepped
/// by.
///
/// Not configurable, because it is the type's rather than a policy: the values
/// between two neighbouring integers are not values the leaf can hold.
pub const WHOLE_STEP: f64 = 1.0;

/// What [`Overrides::vectors`] labels a three-component vector's drag-values
/// with, and the path segments it writes a [`Kind::Struct`] one through.
pub const AXES: [&str; 3] = ["x", "y", "z"];

/// One write an inspector made, as a caller records it.
///
/// Undoing it is `set_path(value, &edit.path, &edit.before)`; redoing it is the
/// same call with [`after`](Self::after).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldEdit {
    /// The dotted [`crcbl_reflect::set_path`] path, from the value the
    /// inspector was handed. Empty when that value is itself the leaf.
    pub path: String,
    /// What the field held before the edit.
    pub before: Value,
    /// What it holds now, read back from the leaf — so a field that narrowed
    /// the write reports what it actually kept.
    pub after: Value,
}

/// What one [`Ui::inspector`] call built, and what it changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Inspection {
    /// The `inspector` block's own [`Response`].
    pub response: Response,
    /// The edits this frame's rows made, in the order they made them; empty
    /// when nothing moved.
    pub edits: Vec<FieldEdit>,
}

/// A row builder registered for one type; see [`Overrides`].
pub type RowBuilder = Box<dyn Fn(&mut Ui, &mut FieldRow<'_>)>;

/// The per-type row builders an inspector draws with instead of its own rows.
///
/// The caller owns this and builds it once — a builder is a boxed closure, so
/// rebuilding the set every frame would allocate every frame. It reaches the
/// widget through [`InspectorOptions::overrides`].
///
/// ```
/// use crcbl_reflect::Value;
/// use crcbl_ui::tree::{Overrides, TextInputOptions, Ui};
///
/// // The vector row this crate ships, plus one of the caller's own: every
/// // `String` field drawn with a placeholder, as Unreal customises an FString.
/// let mut overrides = Overrides::vectors();
/// overrides.register::<String>(|ui, field| {
///     let label = field.label;
///     ui.block(".inspector-row", &[], |ui| {
///         ui.span(".inspector-label", label, &[]);
///         let mut text = match field.get("") {
///             Some(Value::Text(text)) => text,
///             _ => String::new(),
///         };
///         let options = TextInputOptions {
///             placeholder: "unnamed",
///             masked: false,
///         };
///         if ui
///             .text_input_with(".inspector-field", &mut text, options)
///             .changed
///         {
///             field.set("", Value::Text(text));
///         }
///     });
/// });
/// assert_eq!(overrides.len(), 5);
/// ```
#[derive(Default)]
pub struct Overrides {
    /// One builder per type, in registration order. The lookup is linear
    /// because an editor registers a handful of these, not a map's worth.
    builders: Vec<(TypeId, RowBuilder)>,
}

impl fmt::Debug for Overrides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A closure has nothing to print, so what a reader can use is how many
        // types are covered.
        f.debug_struct("Overrides")
            .field("types", &self.builders.len())
            .finish()
    }
}

impl Overrides {
    /// No overrides: every value is drawn by its own [`Kind`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The one override this crate ships: a three-component vector drawn as
    /// three drag-values on one row, registered for `[f64; 3]`, `[f32; 3]`,
    /// [`glam::DVec3`] and [`glam::Vec3`] — the four spellings a position, a
    /// half extent and a linear-RGB tint take in this workspace.
    #[must_use]
    pub fn vectors() -> Self {
        let mut overrides = Self::new();
        overrides.register::<[f64; 3]>(|ui, field| vector_row(ui, field, false));
        overrides.register::<[f32; 3]>(|ui, field| vector_row(ui, field, false));
        overrides.register::<glam::DVec3>(|ui, field| vector_row(ui, field, true));
        overrides.register::<glam::Vec3>(|ui, field| vector_row(ui, field, true));
        overrides
    }

    /// Draws every value of type `T` with `build` rather than with the rows its
    /// [`Kind`] would give. A second registration for one type replaces the
    /// first.
    pub fn register<T: Reflect>(&mut self, build: impl Fn(&mut Ui, &mut FieldRow<'_>) + 'static) {
        let id = TypeId::of::<T>();
        let builder: RowBuilder = Box::new(build);
        match self.builders.iter_mut().find(|(at, _)| *at == id) {
            Some(slot) => slot.1 = builder,
            None => self.builders.push((id, builder)),
        }
    }

    /// How many types are covered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.builders.len()
    }

    /// Whether nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.builders.is_empty()
    }

    /// The builder registered for `value`'s own type, if any.
    fn of(&self, value: &dyn Reflect) -> Option<&RowBuilder> {
        let id = value.as_any().type_id();
        self.builders
            .iter()
            .find(|(at, _)| *at == id)
            .map(|(_, builder)| builder)
    }
}

/// What [`Ui::inspector_with`] takes beside the value.
#[derive(Clone, Copy, Debug, Default)]
pub struct InspectorOptions<'a> {
    /// The step a [`ValueKind::Float`] field with no `#[reflect(step)]` is
    /// dragged and stepped by; [`INSPECTOR_STEP`] when this is not finite or
    /// not positive, which is what [`Default`] leaves it as.
    pub step: f64,
    /// The per-type row builders, or `None` to draw every value by its own
    /// [`Kind`].
    ///
    /// `Option` rather than a borrow of an empty set, because a [`Vec`] has a
    /// destructor and so cannot be promoted to the `'static` an empty default
    /// would have to borrow.
    pub overrides: Option<&'a Overrides>,
}

impl InspectorOptions<'_> {
    /// [`Self::step`], or [`INSPECTOR_STEP`] when it is not a step a drag can
    /// use.
    fn float_step(&self) -> f64 {
        if self.step.is_finite() && self.step > 0.0 {
            self.step
        } else {
            INSPECTOR_STEP
        }
    }
}

/// One row of an inspector, as an override sees it: the value the row is for,
/// what it is labelled and bounded by, and the way an edit is reported.
///
/// Read a leaf with [`get`](Self::get) and write one with [`set`](Self::set),
/// both taking a path **relative to this field** — `""` for the field itself,
/// `"x"` or `"1"` for a component of it.
pub struct FieldRow<'a> {
    /// The value this row draws.
    pub value: &'a mut dyn Reflect,
    /// What the row is labelled with: the field's
    /// [`label`](crcbl_reflect::Field::label), or a list element's index.
    pub label: &'a str,
    /// The interval the field is edited within, from `#[reflect(min, max)]`.
    pub range: Option<Range>,
    /// How far one notch moves it, from `#[reflect(step)]`.
    pub step: Option<f64>,
    /// The dotted path from the inspected value to [`value`](Self::value).
    path: &'a str,
    /// Where an edit this row makes is recorded.
    edits: &'a mut Vec<FieldEdit>,
}

impl fmt::Debug for FieldRow<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldRow")
            .field("type_name", &self.value.type_name())
            .field("label", &self.label)
            .field("range", &self.range)
            .field("step", &self.step)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl FieldRow<'_> {
    /// The dotted path from the inspected value to this row's field.
    #[must_use]
    pub fn path(&self) -> &str {
        self.path
    }

    /// The leaf `at` names inside this field; `""` is the field itself. `None`
    /// when the path names nothing, or stops on something that is not a leaf.
    #[must_use]
    pub fn get(&self, at: &str) -> Option<Value> {
        get_path(self.value, at).ok()
    }

    /// Writes `new` into the leaf `at` names and records the edit under this
    /// row's path joined with `at`.
    ///
    /// Returns whether the field moved. A write the leaf refuses, and one that
    /// landed on the value already there, changes nothing and records nothing.
    pub fn set(&mut self, at: &str, new: Value) -> bool {
        let Ok(before) = get_path(self.value, at) else {
            return false;
        };
        if set_path(self.value, at, &new).is_err() {
            return false;
        }
        let Ok(after) = get_path(self.value, at) else {
            return false;
        };
        if after == before {
            return false;
        }
        self.edits.push(FieldEdit {
            path: joined(self.path, at),
            before,
            after,
        });
        true
    }
}

/// What one row is labelled and bounded by: a [`crcbl_reflect::Field`], or a
/// list element's index.
///
/// Not [`crcbl_reflect::Field`] itself, because its `name` and `label` are
/// `&'static str` and an index's are a [`String`] the frame built.
#[derive(Clone, Copy, Debug)]
struct Row<'a> {
    /// The row's path segment; empty for the inspected value itself.
    name: &'a str,
    /// What the row is labelled with.
    label: &'a str,
    /// The field's `#[reflect(min, max)]`.
    range: Option<Range>,
    /// The field's `#[reflect(step)]`.
    step: Option<f64>,
}

/// `path` with `segment` appended, in `crcbl_reflect::set_path`'s grammar.
fn joined(path: &str, segment: &str) -> String {
    match (path.is_empty(), segment.is_empty()) {
        (_, true) => path.to_owned(),
        (true, false) => segment.to_owned(),
        (false, false) => format!("{path}.{segment}"),
    }
}

/// `value` as the number a drag-value edits.
fn shown(value: &Value) -> f32 {
    match value {
        Value::Int(v) => *v as f32,
        Value::UInt(v) => *v as f32,
        Value::Float(v) => *v as f32,
        Value::Bool(_) | Value::Text(_) => 0.0,
    }
}

/// A dragged `number` back in the leaf's own kind, rounded to the nearest whole
/// number for the two integer kinds; see the module docs.
fn written(kind: ValueKind, number: f32) -> Value {
    match kind {
        ValueKind::Int => Value::Int(number.round() as i64),
        ValueKind::UInt => Value::UInt(number.round() as u64),
        _ => Value::Float(f64::from(number)),
    }
}

/// The interval a drag-value is held inside: the field's when it has one, and
/// the widget's own type otherwise — a field with no `#[reflect(min, max)]`
/// says nothing about what it can hold, so a row must not invent a bound.
fn bounds(range: Option<Range>) -> (f32, f32) {
    range.map_or((f32::MIN, f32::MAX), |range| {
        (range.min as f32, range.max as f32)
    })
}

/// Three drag-values on one row: [`Overrides::vectors`]' builder.
///
/// `named` says how the components are reached — by `x`/`y`/`z` for a glam
/// vector, which is a [`Kind::Struct`], and by index for a `[T; 3]`, which is a
/// [`Kind::List`]. Each is labelled by its axis either way, because the axis a
/// person reads is the axis whatever the path spells it as.
fn vector_row(ui: &mut Ui, field: &mut FieldRow<'_>, named: bool) {
    let step = field.step.unwrap_or(INSPECTOR_STEP) as f32;
    let (min, max) = bounds(field.range);
    let label = field.label;
    ui.block(".inspector-row", &[], |ui| {
        ui.span(".inspector-label", label, &[]);
        for (index, axis) in AXES.iter().enumerate() {
            let segment = if named {
                (*axis).to_owned()
            } else {
                index.to_string()
            };
            let Some(before) = field.get(&segment) else {
                continue;
            };
            let mut number = shown(&before);
            let mut moved = false;
            // Keyed by the axis: three drag-values built at one call site,
            // whose state must follow the component rather than its position.
            ui.block_keyed(*axis, ".inspector-axis", &[], |ui| {
                ui.span(".inspector-axis-label", *axis, &[]);
                moved = ui
                    .drag_value(".inspector-field", &mut number, min..=max, step, step)
                    .changed;
            });
            if moved {
                field.set(&segment, written(before.kind(), number));
            }
        }
    });
}

impl Ui {
    /// A property inspector over `value`: one row per reflected field, as the
    /// module docs describe, with no per-type overrides and [`INSPECTOR_STEP`]
    /// for a float that carries no `#[reflect(step)]`.
    ///
    /// [`Inspection::edits`] is what the frame changed, each as a path and the
    /// value it replaced; the module docs say what undoing one takes.
    #[track_caller]
    pub fn inspector(&mut self, selector: &str, value: &mut dyn Reflect) -> Inspection {
        self.inspector_with(selector, value, &InspectorOptions::default())
    }

    /// [`Ui::inspector`] with `options`: the per-type overrides a caller
    /// registered, and the step an unstepped float takes.
    #[track_caller]
    pub fn inspector_with(
        &mut self,
        selector: &str,
        value: &mut dyn Reflect,
        options: &InspectorOptions<'_>,
    ) -> Inspection {
        let selector = typed("inspector", selector);
        let mut edits = Vec::new();
        let mut path = String::new();
        let response = self.block(&selector, &[], |ui| {
            // A composite is its rows. A leaf handed in on its own, and any
            // value an override covers, is one row of its own instead.
            let root = Row {
                name: "",
                label: value.type_name(),
                range: None,
                step: None,
            };
            let overridden = options
                .overrides
                .is_some_and(|overrides| overrides.of(value).is_some());
            if overridden || matches!(value.kind(), Kind::Leaf(_)) {
                ui.inspect_row(value, root, &mut path, options, &mut edits);
            } else {
                ui.inspect_children(value, root, &mut path, options, &mut edits);
            }
        });
        Inspection { response, edits }
    }

    /// One row per child of `value`: its fields, or its elements by index,
    /// whose bounds are `parent`'s. A leaf has no children and builds nothing.
    fn inspect_children(
        &mut self,
        value: &mut dyn Reflect,
        parent: Row<'_>,
        path: &mut String,
        options: &InspectorOptions<'_>,
        edits: &mut Vec<FieldEdit>,
    ) {
        match value.kind() {
            Kind::Struct | Kind::Enum => {
                // `fields` returns `&'static`, so reading it holds no borrow of
                // `value` across `field_mut`.
                let fields = value.fields();
                for (index, field) in fields.iter().enumerate() {
                    let Some(child) = value.field_mut(index) else {
                        continue;
                    };
                    let row = Row {
                        name: field.name,
                        label: field.label,
                        range: field.range,
                        step: field.step,
                    };
                    self.inspect_row(child, row, path, options, edits);
                }
            }
            Kind::List { len } => {
                for index in 0..len {
                    let name = index.to_string();
                    let Some(child) = value.field_mut(index) else {
                        continue;
                    };
                    // An element has no `Field` of its own, so the list's
                    // bounds are the only ones it can have; see the module
                    // docs.
                    let row = Row {
                        name: &name,
                        label: &name,
                        range: parent.range,
                        step: parent.step,
                    };
                    self.inspect_row(child, row, path, options, edits);
                }
            }
            Kind::Leaf(_) => {}
        }
    }

    /// One row for `child`, under `path` joined with the row's own segment: an
    /// override if one is registered for its type, then a leaf's widget or a
    /// collapsing header holding the value's own rows.
    fn inspect_row(
        &mut self,
        child: &mut dyn Reflect,
        row: Row<'_>,
        path: &mut String,
        options: &InspectorOptions<'_>,
        edits: &mut Vec<FieldEdit>,
    ) {
        let base = path.len();
        if !row.name.is_empty() {
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(row.name);
        }

        if let Some(builder) = options.overrides.and_then(|overrides| overrides.of(child)) {
            let mut field = FieldRow {
                value: child,
                label: row.label,
                range: row.range,
                step: row.step,
                path,
                edits,
            };
            builder(self, &mut field);
        } else {
            match child.kind() {
                Kind::Leaf(kind) => self.leaf_row(child, row, kind, path, options, edits),
                Kind::Struct | Kind::Enum | Kind::List { .. } => {
                    // An enum describes its active variant's fields and no
                    // others, so the header names the variant the rows are of.
                    let title = match child.variant() {
                        Some(variant) => format!("{}: {variant}", row.label),
                        None => row.label.to_owned(),
                    };
                    self.collapsing(".inspector-group", &title, |ui| {
                        ui.inspect_children(child, row, path, options, edits);
                    });
                }
            }
        }

        path.truncate(base);
    }

    /// The row for one leaf: its label, then the widget its [`ValueKind`]
    /// picks. The leaf is written back only in the frame the widget reports a
    /// change; see the module docs.
    fn leaf_row(
        &mut self,
        child: &mut dyn Reflect,
        row: Row<'_>,
        kind: ValueKind,
        path: &str,
        options: &InspectorOptions<'_>,
        edits: &mut Vec<FieldEdit>,
    ) {
        let Some(before) = child.get() else {
            return;
        };
        let float_step = options.float_step();
        self.block(".inspector-row", &[], |ui| {
            ui.span(".inspector-label", row.label, &[]);
            let new = match kind {
                ValueKind::Bool => {
                    let mut on = matches!(before, Value::Bool(true));
                    ui.checkbox(".inspector-field", "", &mut on)
                        .changed
                        .then_some(Value::Bool(on))
                }
                ValueKind::Text => {
                    let mut text = match &before {
                        Value::Text(text) => text.clone(),
                        _ => String::new(),
                    };
                    ui.text_input(".inspector-field", &mut text)
                        .changed
                        .then_some(Value::Text(text))
                }
                ValueKind::Int | ValueKind::UInt | ValueKind::Float => {
                    let step = row.step.unwrap_or(match kind {
                        ValueKind::Float => float_step,
                        _ => WHOLE_STEP,
                    }) as f32;
                    let (min, max) = bounds(row.range);
                    let mut number = shown(&before);
                    ui.drag_value(".inspector-field", &mut number, min..=max, step, step)
                        .changed
                        .then(|| written(kind, number))
                }
            };
            if let Some(new) = new {
                write_leaf(child, path, &new, edits);
            }
        });
    }
}

/// Writes `new` into `leaf` and records what it replaced.
///
/// Nothing is recorded when the leaf refuses the write, or when the value it
/// kept is the one it already held — a narrowing leaf can turn a changed widget
/// into an unchanged field, and an edit nobody can see is not one a caller
/// should have to undo.
fn write_leaf(leaf: &mut dyn Reflect, path: &str, new: &Value, edits: &mut Vec<FieldEdit>) {
    let Some(before) = leaf.get() else {
        return;
    };
    if leaf.set(new).is_err() {
        return;
    }
    let Some(after) = leaf.get() else {
        return;
    };
    if after == before {
        return;
    }
    edits.push(FieldEdit {
        path: path.to_owned(),
        before,
        after,
    });
}
