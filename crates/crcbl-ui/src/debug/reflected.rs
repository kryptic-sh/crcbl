//! A reflected value as a debug panel section: one `label: value` row per leaf.
//!
//! This is how a selected entity's data reaches the panel. A system lends its
//! row as `&dyn Reflect` — `crcbl_ecs::SystemTrait::debug_fields` — and
//! [`ReflectedSection`] walks it with the same [`Reflect::fields`] labels the
//! property inspector (`Ui::inspector`) draws, so a component reads the same in
//! the F3 panel as in the editor. It is **read-only**: the panel is rows of
//! text, and the inspector's widgets edit a `&mut dyn Reflect` the panel is
//! never handed.
//!
//! # Which rows a value becomes
//!
//! * A leaf is one row: a float to [`REFLECTED_DECIMALS`] places, everything
//!   else as [`Value`]'s `Display` spells it.
//! * A struct's fields are rows labelled by their path from the section —
//!   `half extents.x` — with each segment the field's
//!   [`label`](crcbl_reflect::Field::label).
//! * An enum is a row naming its active variant, then that variant's fields.
//! * A list of leaves is **one** row, its elements comma-separated: a position
//!   reads `1.000, 2.000, 3.000` rather than three rows labelled `0`, `1`, `2`.
//!   A list of composites is a row group per element, labelled by index.
//!
//! A value that is itself a leaf — a `System<f32>`'s row — is labelled with its
//! [`type_name`](Reflect::type_name), as it has no field name to carry.

use core::fmt;

use crcbl_reflect::{Kind, Reflect, Value};

use super::{DebugModule, DebugSection};

/// How many decimal places a float in a [`ReflectedSection`] shows.
///
/// A reflected float arrives widened to `f64`, so an `f32` field printed in
/// full is seventeen digits of rounding noise — `0.30000001192092896` for a
/// field set to `0.3`. Three places are millimetres in a metre-scale world,
/// which is what a developer reading a live value can use.
pub const REFLECTED_DECIMALS: usize = 3;

/// What an enum's variant row is labelled with when the enum is the whole
/// value, and so has no field name of its own.
const VARIANT_LABEL: &str = "variant";

/// Separates the segments of a nested row's label.
const PATH_SEPARATOR: char = '.';

/// A [`DebugModule`] for any [`Reflect`] value: a section titled `title` with
/// one row per leaf, labelled by its path of field labels.
///
/// An enum adds a row naming its active variant; a list of leaves is one
/// comma-separated row; a float shows [`REFLECTED_DECIMALS`] places; a value
/// that is itself a leaf is labelled with its type's name. Read-only: the
/// panel is never handed the `&mut dyn Reflect` an edit would need.
#[derive(Clone, Copy)]
pub struct ReflectedSection<'a> {
    /// The section's heading — a system's name, for a selected entity.
    pub title: &'a str,
    /// The value its rows describe.
    pub value: &'a dyn Reflect,
}

impl fmt::Debug for ReflectedSection<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReflectedSection")
            .field("title", &self.title)
            .field("type", &self.value.type_name())
            .finish()
    }
}

impl DebugModule for ReflectedSection<'_> {
    fn debug_section(&self, out: &mut DebugSection) {
        out.set_title(self.title);
        let mut label = String::new();
        rows(out, &mut label, self.value);
    }
}

/// Writes `value`'s rows into `out`, each labelled `label` plus its own path.
///
/// `label` is the path so far and is left as it was found; an empty one means
/// `value` is the section's whole value.
fn rows(out: &mut DebugSection, label: &mut String, value: &dyn Reflect) {
    match value.kind() {
        Kind::Leaf(_) => {
            let shown = if label.is_empty() {
                value.type_name()
            } else {
                label.as_str()
            };
            out.row(shown, format_args!("{}", Leaf(value)));
        }
        Kind::Struct => fields(out, label, value),
        Kind::Enum => {
            let shown = if label.is_empty() {
                VARIANT_LABEL
            } else {
                label.as_str()
            };
            out.row_str(shown, value.variant().unwrap_or_default());
            fields(out, label, value);
        }
        Kind::List { len } => {
            let elements = (0..len).filter_map(|index| value.field(index));
            if elements
                .clone()
                .all(|element| matches!(element.kind(), Kind::Leaf(_)))
            {
                let shown = if label.is_empty() {
                    value.type_name()
                } else {
                    label.as_str()
                };
                out.row(shown, format_args!("{}", Joined(value, len)));
            } else {
                for (index, element) in elements.enumerate() {
                    nested(out, label, format_args!("{index}"), element);
                }
            }
        }
    }
}

/// The rows of a struct's fields, or of an enum's active variant's.
fn fields(out: &mut DebugSection, label: &mut String, value: &dyn Reflect) {
    for (index, field) in value.fields().iter().enumerate() {
        if let Some(child) = value.field(index) {
            nested(out, label, format_args!("{}", field.label), child);
        }
    }
}

/// [`rows`] for `child`, with `segment` appended to `label` for the call and
/// taken off again after it.
fn nested(
    out: &mut DebugSection,
    label: &mut String,
    segment: fmt::Arguments<'_>,
    child: &dyn Reflect,
) {
    let kept = label.len();
    if kept > 0 {
        label.push(PATH_SEPARATOR);
    }
    // Writing into a `String` cannot fail; the `Result` is `fmt`'s shape.
    let _ = fmt::Write::write_fmt(label, segment);
    rows(out, label, child);
    label.truncate(kept);
}

/// A leaf's value as a row prints it; see [`REFLECTED_DECIMALS`].
struct Leaf<'a>(&'a dyn Reflect);

impl fmt::Display for Leaf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.get() {
            Some(Value::Float(value)) => write!(f, "{value:.REFLECTED_DECIMALS$}"),
            Some(value) => write!(f, "{value}"),
            // A `Kind::Leaf` with no value breaks `Reflect`'s own contract;
            // the row says so rather than printing a value it does not have.
            None => write!(f, "<{} has no value>", self.0.type_name()),
        }
    }
}

/// A list of leaves, comma-separated, as one row's value.
struct Joined<'a>(&'a dyn Reflect, usize);

impl fmt::Display for Joined<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(list, len) = *self;
        for (index, element) in (0..len).filter_map(|index| list.field(index)).enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", Leaf(element))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::debug::DebugOverlay;
    use crate::draw_list::{DrawCommand, DrawList};
    use crate::text::FontAtlas;

    #[derive(Reflect)]
    enum Mode {
        Idle,
        Chasing { speed: f32 },
    }

    #[derive(Reflect)]
    struct Offset {
        x: f64,
    }

    /// Every shape [`rows`] distinguishes: a renamed leaf, a list of leaves, a
    /// nested struct, an enum and a list of composites.
    #[derive(Reflect)]
    struct Guard {
        #[reflect(name = "Hit points")]
        hit_points: i32,
        position: [f32; 3],
        offset: Offset,
        mode: Mode,
        waypoints: [Offset; 2],
        awake: bool,
    }

    fn guard() -> Guard {
        Guard {
            hit_points: 12,
            position: [1.0, 0.3, -2.5],
            offset: Offset { x: 0.125 },
            mode: Mode::Chasing { speed: 4.5 },
            waypoints: [Offset { x: 1.0 }, Offset { x: 2.0 }],
            awake: true,
        }
    }

    /// The `(label, value)` rows `value` becomes under `title`.
    fn rows_of(title: &str, value: &dyn Reflect) -> Vec<(String, String)> {
        let mut section = DebugSection::default();
        ReflectedSection { title, value }.debug_section(&mut section);
        assert_eq!(section.title(), title);
        section
            .rows()
            .iter()
            .map(|row| (row.label.clone(), row.value.clone()))
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        expected
            .iter()
            .map(|&(label, value)| (label.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    fn a_struct_becomes_a_row_per_leaf_labelled_by_its_path() {
        assert_eq!(
            rows_of("guards", &guard()),
            pairs(&[
                ("Hit points", "12"),
                ("position", "1.000, 0.300, -2.500"),
                ("offset.x", "0.125"),
                ("mode", "Chasing"),
                ("mode.speed", "4.500"),
                ("waypoints.0.x", "1.000"),
                ("waypoints.1.x", "2.000"),
                ("awake", "true"),
            ]),
        );
    }

    #[test]
    fn a_value_with_no_field_name_is_labelled_by_its_type() {
        assert_eq!(rows_of("speed", &2.0f32), pairs(&[("f32", "2.000")]));
        assert_eq!(
            rows_of("tint", &[0.5f32, 0.25, 1.0]),
            pairs(&[("[f32; 3]", "0.500, 0.250, 1.000")]),
        );
        assert_eq!(rows_of("mode", &Mode::Idle), pairs(&[("variant", "Idle")]));
    }

    /// The section reaches the panel's draw list like any other module's: the
    /// title, then each label beside its value.
    #[test]
    fn a_reflected_section_is_drawn_in_the_panel() {
        let atlas = FontAtlas::built_in();
        let mut overlay = DebugOverlay::with_visible(true);
        overlay.begin_frame();
        overlay.add(&ReflectedSection {
            title: "guards",
            value: &guard(),
        });

        let mut dl = DrawList::new();
        overlay.render(&mut dl, Vec2::new(1280.0, 720.0), &atlas);
        let drawn: Vec<String> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        let at = drawn
            .iter()
            .position(|text| text == "guards")
            .unwrap_or_else(|| panic!("the section's title: {drawn:?}"));
        assert_eq!(
            drawn[at + 1..at + 5],
            ["Hit points", "12", "position", "1.000, 0.300, -2.500"],
            "each label is followed by its value: {drawn:?}",
        );
    }
}
