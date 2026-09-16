//! What a leaf holds, and how a write into one is refused.
//!
//! An inspector row is either something a widget edits — a number, a flag, a
//! line of text — or something it descends into. This module is the first half:
//! the payload [`Value`], the tag [`ValueKind`] it is drawn from, and the
//! [`SetError`] a write back is refused with.

use core::fmt;

/// What a leaf holds, without the payload.
///
/// The tag an inspector picks a widget from: a checkbox for [`Bool`](Self::Bool),
/// a drag-value for the two integer kinds and [`Float`](Self::Float), a text box
/// for [`Text`](Self::Text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueKind {
    /// `bool`.
    Bool,
    /// A signed whole number: `i8` through `i64`, and `isize`.
    Int,
    /// An unsigned whole number: `u8` through `u64`, and `usize`.
    UInt,
    /// `f32` or `f64`.
    Float,
    /// `String`.
    Text,
}

impl ValueKind {
    /// How the kind is named in a refusal, e.g. `"a whole number"`.
    ///
    /// The article is included so a message reads as a sentence, which is the
    /// shape `crcbl_console::Kind::article_name` uses for the same job.
    #[must_use]
    pub const fn article_name(self) -> &'static str {
        match self {
            Self::Bool => "a flag",
            Self::Int => "a whole number",
            Self::UInt => "a count",
            Self::Float => "a number",
            Self::Text => "text",
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.article_name())
    }
}

/// A leaf's value: what an inspector reads out of a field and writes back into
/// it, and what an edit command carries.
///
/// **The float arm is `f64`, not `f32`**, and that is the one place this type
/// deliberately differs from `crcbl_console::Value`, whose shape every other
/// part of it follows. Positions in this engine are `f64` — `crcbl_core::WorldPos`'s
/// local offset, `crcbl::phys::Transform`, and the `[f64; 3]` rows a
/// `.scn/` chunk file holds — and `apps/breakout/src/scene.rs` says in as many
/// words that a board written as `f32` "would round on the way through the file
/// and move the picture". A leaf that narrowed on the way to the inspector and
/// widened on the way back would move a brick every time a panel was opened.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A [`ValueKind::Bool`] value.
    Bool(bool),
    /// A [`ValueKind::Int`] value, widened to the largest signed type.
    Int(i64),
    /// A [`ValueKind::UInt`] value, widened to the largest unsigned type.
    UInt(u64),
    /// A [`ValueKind::Float`] value, widened to the wider of the two floats.
    Float(f64),
    /// A [`ValueKind::Text`] value.
    Text(String),
}

impl Value {
    /// The tag this value was drawn from.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            Self::Bool(_) => ValueKind::Bool,
            Self::Int(_) => ValueKind::Int,
            Self::UInt(_) => ValueKind::UInt,
            Self::Float(_) => ValueKind::Float,
            Self::Text(_) => ValueKind::Text,
        }
    }
}

/// Prints in the form a person reads in a row, with text left bare.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::UInt(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Text(v) => f.write_str(v),
        }
    }
}

/// Why a write into a leaf was refused.
///
/// Every arm names the type it is about, so a panel can say which row rejected
/// the edit without tracking that itself.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SetError {
    /// The value is not of the kind this leaf holds.
    #[error("`{type_name}` is {expected}, not {actual}")]
    Kind {
        /// The leaf's type.
        type_name: &'static str,
        /// What it holds.
        expected: ValueKind,
        /// What it was handed.
        actual: ValueKind,
    },

    /// The value is of the right kind and does not fit in this leaf.
    ///
    /// A [`Value::Int`] of `300` into an `i8`, or a [`Value::Float`] whose
    /// magnitude is past `f32::MAX` into an `f32` — where the `as` cast would
    /// saturate to an infinity and call it a write.
    #[error("`{type_name}` cannot hold {value}")]
    Range {
        /// The leaf's type.
        type_name: &'static str,
        /// What it was handed.
        value: Value,
    },

    /// A non-finite float, refused for every float leaf.
    ///
    /// The same refusal `crcbl_console::Kind::check` makes: a NaN written into a
    /// transform is a corruption no later edit can undo, and an inspector has no
    /// use for one.
    #[error("`{type_name}` cannot hold {value}, which is not finite")]
    NotFinite {
        /// The leaf's type.
        type_name: &'static str,
        /// What it was handed.
        value: f64,
    },

    /// The target is not a leaf: it has fields, elements or a variant instead.
    #[error("`{type_name}` has fields rather than a value of its own")]
    NotALeaf {
        /// The composite's type.
        type_name: &'static str,
    },
}

/// What a reflected value is made of, which decides how an inspector draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A value a widget edits directly. [`Reflect::get`] and [`Reflect::set`]
    /// are the two halves; [`Reflect::fields`] is empty.
    ///
    /// [`Reflect::get`]: crate::Reflect::get
    /// [`Reflect::set`]: crate::Reflect::set
    /// [`Reflect::fields`]: crate::Reflect::fields
    Leaf(ValueKind),

    /// Named fields, described by [`Reflect::fields`] and reached by
    /// [`Reflect::field`].
    ///
    /// [`Reflect::fields`]: crate::Reflect::fields
    /// [`Reflect::field`]: crate::Reflect::field
    Struct,

    /// One variant of several. [`Reflect::variant`] names the active one and
    /// [`Reflect::fields`] describes **that variant's** fields.
    ///
    /// [`Reflect::variant`]: crate::Reflect::variant
    /// [`Reflect::fields`]: crate::Reflect::fields
    Enum,

    /// `len` elements addressed by index rather than by name, so
    /// [`Reflect::fields`] is empty and [`Reflect::field`] takes the index.
    ///
    /// [`Reflect::fields`]: crate::Reflect::fields
    /// [`Reflect::field`]: crate::Reflect::field
    List {
        /// How many elements there are.
        len: usize,
    },
}

/// The closed interval a numeric field is edited within.
///
/// Both ends are `f64` whatever the field's own type is, for the reason
/// [`Value::Float`] is: the widest of the leaf types has to be able to say what
/// it means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    /// The smallest accepted value, inclusive.
    pub min: f64,
    /// The largest accepted value, inclusive.
    pub max: f64,
}

/// One row of an inspector: a field of a struct, or of an enum's active variant.
///
/// Everything here is `'static` because it is written by `#[derive(Reflect)]`
/// into the expansion, not computed per value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    /// The field's name in the source, which is also its path segment.
    pub name: &'static str,

    /// What the row is labelled with: `#[reflect(name = "…")]` when the field
    /// carries one, and [`name`](Self::name) when it does not.
    pub label: &'static str,

    /// The interval a drag-value is clamped to, from `#[reflect(min = …, max =
    /// …)]`.
    ///
    /// **Advisory, not enforced.** It is what a widget bounds its drag by;
    /// [`Reflect::set`] does not read it, because the field's own type is the
    /// only thing that can refuse a value on its own behalf and a path
    /// resolution reaches that leaf without passing this row.
    ///
    /// [`Reflect::set`]: crate::Reflect::set
    pub range: Option<Range>,

    /// How far one notch of a drag-value moves the field, from
    /// `#[reflect(step = …)]`. Advisory, like [`range`](Self::range).
    pub step: Option<f64>,
}

impl Field {
    /// A row with no label override, no range and no step.
    ///
    /// The shape every built-in composite's rows have — only a derived type can
    /// carry an attribute.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            label: name,
            range: None,
            step: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_value_reports_the_kind_it_was_drawn_from() {
        assert_eq!(Value::Bool(true).kind(), ValueKind::Bool);
        assert_eq!(Value::Int(-1).kind(), ValueKind::Int);
        assert_eq!(Value::UInt(1).kind(), ValueKind::UInt);
        assert_eq!(Value::Float(1.0).kind(), ValueKind::Float);
        assert_eq!(Value::Text("x".into()).kind(), ValueKind::Text);
    }

    #[test]
    fn a_kind_mismatch_names_both_kinds_in_words() {
        let error = SetError::Kind {
            type_name: "f32",
            expected: ValueKind::Float,
            actual: ValueKind::Text,
        };
        assert_eq!(error.to_string(), "`f32` is a number, not text");
    }

    #[test]
    fn an_out_of_range_write_names_the_type_and_the_value() {
        let error = SetError::Range {
            type_name: "i8",
            value: Value::Int(300),
        };
        assert_eq!(error.to_string(), "`i8` cannot hold 300");
    }

    #[test]
    fn a_non_finite_write_says_so() {
        let error = SetError::NotFinite {
            type_name: "f64",
            value: f64::INFINITY,
        };
        assert_eq!(
            error.to_string(),
            "`f64` cannot hold inf, which is not finite"
        );
    }

    #[test]
    fn a_composite_refuses_a_write_by_saying_it_has_fields() {
        let error = SetError::NotALeaf { type_name: "Brick" };
        assert_eq!(
            error.to_string(),
            "`Brick` has fields rather than a value of its own"
        );
    }

    #[test]
    fn text_prints_bare_and_numbers_print_as_themselves() {
        assert_eq!(Value::Text("a b".into()).to_string(), "a b");
        assert_eq!(Value::Float(1.5).to_string(), "1.5");
        assert_eq!(Value::Int(-7).to_string(), "-7");
        assert_eq!(Value::UInt(7).to_string(), "7");
        assert_eq!(Value::Bool(false).to_string(), "false");
    }

    #[test]
    fn a_field_with_no_attribute_is_labelled_by_its_own_name() {
        let field = Field::new("half_extents");
        assert_eq!(field.label, "half_extents");
        assert_eq!(field.range, None);
        assert_eq!(field.step, None);
    }
}
