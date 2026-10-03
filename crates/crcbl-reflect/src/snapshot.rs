//! A whole value read out so it can be written back exactly — the inverse an
//! enum's variant switch needs.
//!
//! [`get_path`](crate::get_path) reads one leaf, and that is all a leaf edit's
//! inverse is. A variant switch replaces **every** field of the variant it
//! leaves with the next variant's defaults, so its inverse has to hold the old
//! variant's name and everything under it, nested enums by their own variant.
//! That is a [`Snapshot`]: a tree of the value's leaves in
//! [`Reflect::field`] order, each composite tagged by the variant it was in.
//!
//! It is **not a serialisation**. It holds no type and no field names, only
//! positions, so it is written back into the value it was read from — the same
//! path of the same type — and refused ([`SetError::Shape`]) where it does not
//! fit. The scene format remains the one a value is saved through.
//!
//! **It holds the rows, and only the rows.** A field `#[reflect(skip)]` leaves
//! off the panel is in no snapshot, so a restore that switches an enum back to
//! a variant holding one makes it at its type's `Default`, as any switch does;
//! a restore within the active variant leaves it alone.

use std::borrow::Cow;

use crate::path::{resolve, resolve_mut};
use crate::{Kind, PathError, Reflect, SetError, Value};

/// Everything a reflected value holds, read by [`Snapshot::of`] and written
/// back by [`Snapshot::restore`].
///
/// `PartialEq` is `Value`'s, floats compared as numbers rather than bits.
#[derive(Clone, Debug, PartialEq)]
pub enum Snapshot {
    /// A leaf's value.
    Leaf(Value),
    /// A struct's fields or a list's elements, in [`Reflect::field`] order.
    Fields(Vec<Snapshot>),
    /// An enum: the variant that was active, and its fields in order.
    Variant {
        /// The variant's name: borrowed as [`Reflect::variant`] answered it
        /// for a snapshot read off a value, and owned for one read off bytes
        /// — an edit's wire form, or the edit history beside a scene — where
        /// no type is there to borrow it from until it is restored.
        name: Cow<'static, str>,
        /// Its fields, in [`Reflect::field`] order.
        fields: Vec<Snapshot>,
    },
}

impl Snapshot {
    /// Reads everything `value` holds: its own value if it is a leaf, and
    /// otherwise every child [`Reflect::field`] reaches, with the active
    /// variant for an enum.
    #[must_use]
    pub fn of(value: &dyn Reflect) -> Self {
        if let Some(leaf) = value.get() {
            return Self::Leaf(leaf);
        }
        let children = (0..children_of(value))
            .filter_map(|index| value.field(index))
            .map(Self::of)
            .collect();
        match value.variant() {
            Some(name) => Self::Variant {
                name: Cow::Borrowed(name),
                fields: children,
            },
            None => Self::Fields(children),
        }
    }

    /// Writes this snapshot back into `value`: each enum switched to the
    /// variant it was read in, then each leaf set. A leaf already holding the
    /// value it is given is not written, so a value restored over itself
    /// changes nothing — a non-finite float no write would take included.
    ///
    /// # Errors
    ///
    /// [`SetError::Shape`] where the snapshot does not fit `value`,
    /// [`SetError::NoVariant`] for a variant `value`'s type does not have, and
    /// a leaf's own refusal. A refusal part-way puts back what `value` held,
    /// read before the first write; if even that is refused — which only a
    /// leaf holding a non-finite float, inside a variant the refused write had
    /// switched away from, can do — that refusal is the one returned, and the
    /// leaf keeps its variant's default.
    pub fn restore(&self, value: &mut dyn Reflect) -> Result<(), SetError> {
        let held = Self::of(value);
        self.write(value).or_else(|refusal| {
            held.write(value)?;
            Err(refusal)
        })
    }

    /// [`restore`](Self::restore)'s walk, without the put-back.
    fn write(&self, value: &mut dyn Reflect) -> Result<(), SetError> {
        match self {
            Self::Leaf(leaf) => {
                if value.get().is_some_and(|held| held.identical(leaf)) {
                    return Ok(());
                }
                value.set(leaf)
            }
            Self::Fields(children) => {
                if value.variant().is_some() {
                    return Err(SetError::Shape {
                        type_name: value.type_name(),
                    });
                }
                write_children(value, children)
            }
            Self::Variant { name, fields } => {
                // A variant the type lists is checked for its row count before
                // the switch, so the one misfit the description can see never
                // costs the variant being left.
                if value
                    .variants()
                    .iter()
                    .any(|variant| variant.name == *name && variant.fields.len() != fields.len())
                {
                    return Err(SetError::Shape {
                        type_name: value.type_name(),
                    });
                }
                value.set_variant(name)?;
                write_children(value, fields)
            }
        }
    }
}

/// How many children [`Reflect::field`] reaches in `value`: a list's length,
/// or the rows [`Reflect::fields`] describes.
fn children_of(value: &dyn Reflect) -> usize {
    match value.kind() {
        Kind::List { len } => len,
        Kind::Struct | Kind::Enum | Kind::Leaf(_) => value.fields().len(),
    }
}

/// Writes `children` into `value`'s children in order, refusing a composite
/// with another number of them — or a leaf, which has none to write.
fn write_children(value: &mut dyn Reflect, children: &[Snapshot]) -> Result<(), SetError> {
    let shape = SetError::Shape {
        type_name: value.type_name(),
    };
    if matches!(value.kind(), Kind::Leaf(_)) || children_of(value) != children.len() {
        return Err(shape);
    }
    for (index, child) in children.iter().enumerate() {
        let Some(at) = value.field_mut(index) else {
            return Err(shape);
        };
        child.write(at)?;
    }
    Ok(())
}

/// Reads everything under the value `path` names inside `value` — a leaf, a
/// struct, a list or an enum — as [`Snapshot::of`] does.
///
/// This is the **before** half of an undoable variant switch, as
/// [`get_path`](crate::get_path) is of a leaf edit.
///
/// # Errors
///
/// [`PathError::NoField`] if a segment names nothing.
///
/// ```
/// use crcbl_reflect::{Snapshot, Value, snapshot_path};
///
/// let grid = [[1.0_f64, 2.0], [3.0, 4.0]];
/// assert_eq!(
///     snapshot_path(&grid, "1"),
///     Ok(Snapshot::Fields(vec![
///         Snapshot::Leaf(Value::Float(3.0)),
///         Snapshot::Leaf(Value::Float(4.0)),
///     ])),
/// );
/// ```
pub fn snapshot_path(value: &dyn Reflect, path: &str) -> Result<Snapshot, PathError> {
    Ok(Snapshot::of(resolve(value, path)?))
}

/// Writes `snapshot` back into the value `path` names inside `value`, as
/// [`Snapshot::restore`] does.
///
/// This is how an undoable variant switch is applied and undone: each
/// direction is the snapshot of the state it leads to.
///
/// # Errors
///
/// [`PathError::NoField`] if a segment names nothing, or [`PathError::Set`]
/// carrying [`Snapshot::restore`]'s refusal.
pub fn restore_path(
    value: &mut dyn Reflect,
    path: &str,
    snapshot: &Snapshot,
) -> Result<(), PathError> {
    snapshot.restore(resolve_mut(value, path)?)?;
    Ok(())
}
