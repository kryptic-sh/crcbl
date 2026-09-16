//! Reaching one leaf inside a value by name, which is what an edit command
//! carries.
//!
//! A property panel walks a component with [`Reflect::fields`] and
//! [`Reflect::field`] and knows where it is; a **command** does not. It is
//! recorded when the edit is made and applied — or undone — later, against a
//! value it never walked, so it carries a path instead of a borrow.
//!
//! The grammar is one line: segments separated by `.`, each segment either a
//! [`Field::name`] (for a [`Kind::Struct`] or the active variant of a
//! [`Kind::Enum`]) or a decimal index (for a [`Kind::List`]). `"position.1"` is
//! a brick's `y`; `"shape.width"` is a platform's width. The empty path is the
//! value itself.
//!
//! [`Reflect::fields`]: crate::Reflect::fields
//! [`Reflect::field`]: crate::Reflect::field
//! [`Field::name`]: crate::Field::name
//! [`Kind::Struct`]: crate::Kind::Struct
//! [`Kind::Enum`]: crate::Kind::Enum
//! [`Kind::List`]: crate::Kind::List

use crate::{Kind, Reflect, SetError, Value};

/// Why a path did not reach a leaf.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PathError {
    /// A segment named nothing in the value it was resolved against.
    ///
    /// For an enum this is also what a segment naming a field of some **other**
    /// variant gets: only the active variant has fields.
    #[error("`{type_name}` has no field `{segment}`")]
    NoField {
        /// The type the segment was resolved against.
        type_name: &'static str,
        /// The segment, as it was written.
        segment: String,
    },

    /// The path ran out at a value that has fields rather than one of its own.
    #[error("`{type_name}` has fields rather than a value of its own")]
    NotALeaf {
        /// The type the path stopped at.
        type_name: &'static str,
    },

    /// The leaf was reached and refused the write.
    #[error(transparent)]
    Set(#[from] SetError),
}

/// Walks `path` from `value` and returns what it lands on.
fn resolve<'a>(value: &'a dyn Reflect, path: &str) -> Result<&'a dyn Reflect, PathError> {
    let mut at = value;
    for segment in segments(path) {
        at = index_of(at, segment)
            .and_then(|index| at.field(index))
            .ok_or_else(|| PathError::NoField {
                type_name: at.type_name(),
                segment: segment.to_owned(),
            })?;
    }
    Ok(at)
}

/// [`resolve`] for a write.
///
/// A second walk rather than a shared one: the borrow checker cannot express
/// "the same traversal, either shared or unique" without either a macro over the
/// mutability or an unsafe reborrow, and both cost more than sixteen lines. The
/// two bodies are held together by the tests, which assert the same paths
/// resolve through both.
fn resolve_mut<'a>(
    value: &'a mut dyn Reflect,
    path: &str,
) -> Result<&'a mut dyn Reflect, PathError> {
    let mut at = value;
    for segment in segments(path) {
        // Both the index and the name for a refusal are taken while the borrow
        // is still shared; `field_mut` is what consumes `at`.
        let type_name = at.type_name();
        let no_field = || PathError::NoField {
            type_name,
            segment: segment.to_owned(),
        };
        let index = index_of(at, segment).ok_or_else(no_field)?;
        at = at.field_mut(index).ok_or_else(no_field)?;
    }
    Ok(at)
}

/// The segments of `path`, with the empty path yielding none.
fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('.').filter(|segment| !segment.is_empty())
}

/// Which child of `at` a path segment names, if any.
///
/// A list takes a decimal index and nothing else; a struct or an enum takes a
/// [`crate::Field::name`]. A struct is **not** also indexable by number: a path
/// that resolved two ways would be two paths for one field, and the recorded
/// form of an edit command should have one spelling.
fn index_of(at: &dyn Reflect, segment: &str) -> Option<usize> {
    match at.kind() {
        Kind::List { len } => segment.parse::<usize>().ok().filter(|index| *index < len),
        Kind::Struct | Kind::Enum => at.fields().iter().position(|field| field.name == segment),
        Kind::Leaf(_) => None,
    }
}

/// Reads the leaf `path` names inside `value`.
///
/// This is how the **before** half of an undoable edit is captured: a command
/// records what the field held, and applying its inverse is [`set_path`] with
/// that value.
///
/// # Errors
///
/// [`PathError::NoField`] if a segment names nothing, or
/// [`PathError::NotALeaf`] if the path stops at a struct, an enum or a list.
///
/// ```
/// use crcbl_reflect::{Value, get_path};
///
/// let brick = [1.0_f64, 2.0, 3.0];
/// assert_eq!(get_path(&brick, "1"), Ok(Value::Float(2.0)));
/// ```
pub fn get_path(value: &dyn Reflect, path: &str) -> Result<Value, PathError> {
    let at = resolve(value, path)?;
    at.get().ok_or(PathError::NotALeaf {
        type_name: at.type_name(),
    })
}

/// Writes `new` into the leaf `path` names inside `value`.
///
/// This is how an edit command is applied, and how its inverse is applied when
/// it is undone.
///
/// # Errors
///
/// [`PathError::NoField`] if a segment names nothing,
/// [`PathError::NotALeaf`] if the path stops at a composite, or
/// [`PathError::Set`] carrying the leaf's own refusal.
///
/// ```
/// use crcbl_reflect::{Value, get_path, set_path};
///
/// let mut brick = [1.0_f64, 2.0, 3.0];
/// set_path(&mut brick, "1", &Value::Float(7.5)).unwrap();
/// assert_eq!(get_path(&brick, "1"), Ok(Value::Float(7.5)));
/// ```
pub fn set_path(value: &mut dyn Reflect, path: &str, new: &Value) -> Result<(), PathError> {
    let at = resolve_mut(value, path)?;
    if matches!(at.kind(), Kind::Leaf(_)) {
        at.set(new)?;
        Ok(())
    } else {
        Err(PathError::NotALeaf {
            type_name: at.type_name(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_path_is_the_value_itself() {
        let mut value = 3.5_f64;
        assert_eq!(get_path(&value, ""), Ok(Value::Float(3.5)));
        set_path(&mut value, "", &Value::Float(4.0)).expect("a leaf takes its own kind");
        assert_eq!(value, 4.0);
    }

    #[test]
    fn a_list_is_addressed_by_decimal_index() {
        let position = [1.0_f64, 2.0, 3.0];
        assert_eq!(get_path(&position, "0"), Ok(Value::Float(1.0)));
        assert_eq!(get_path(&position, "2"), Ok(Value::Float(3.0)));
    }

    #[test]
    fn an_index_past_the_end_names_the_type_and_the_segment() {
        let position = [1.0_f64, 2.0, 3.0];
        assert_eq!(
            get_path(&position, "3"),
            Err(PathError::NoField {
                type_name: "[f64; 3]",
                segment: "3".to_owned(),
            })
        );
    }

    #[test]
    fn a_list_refuses_a_name_and_a_struct_refuses_a_number() {
        let position = [1.0_f64, 2.0];
        assert!(matches!(
            get_path(&position, "x"),
            Err(PathError::NoField { .. })
        ));

        let vector = glam::Vec2::new(1.0, 2.0);
        assert_eq!(get_path(&vector, "x"), Ok(Value::Float(1.0)));
        assert!(matches!(
            get_path(&vector, "0"),
            Err(PathError::NoField { .. })
        ));
    }

    #[test]
    fn a_path_that_stops_on_a_composite_is_refused_by_both_halves() {
        let mut position = [1.0_f64, 2.0];
        assert_eq!(
            get_path(&position, ""),
            Err(PathError::NotALeaf {
                type_name: "[f64; 2]"
            })
        );
        assert_eq!(
            set_path(&mut position, "", &Value::Float(1.0)),
            Err(PathError::NotALeaf {
                type_name: "[f64; 2]"
            })
        );
    }

    #[test]
    fn a_leafs_own_refusal_travels_out_through_the_path() {
        let mut counts = [1_u8, 2];
        assert_eq!(
            set_path(&mut counts, "0", &Value::UInt(300)),
            Err(PathError::Set(SetError::Range {
                type_name: "u8",
                value: Value::UInt(300),
            }))
        );
        assert_eq!(counts[0], 1, "a refused write leaves the field alone");
    }

    #[test]
    fn nested_lists_resolve_through_every_segment() {
        let mut grid = [[1.0_f32, 2.0], [3.0, 4.0]];
        assert_eq!(get_path(&grid, "1.0"), Ok(Value::Float(3.0)));
        set_path(&mut grid, "1.0", &Value::Float(9.0)).expect("the leaf takes a float");
        assert_eq!(grid[1][0], 9.0);
    }

    #[test]
    fn a_glam_vector_resolves_and_writes_by_component_name() {
        let mut vector = glam::DVec3::new(1.0, 2.0, 3.0);
        assert_eq!(get_path(&vector, "z"), Ok(Value::Float(3.0)));
        set_path(&mut vector, "y", &Value::Float(-1.0)).expect("a component is an f64");
        assert_eq!(vector.y, -1.0);
    }
}
