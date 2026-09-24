//! What an editor's property panel needs to know about a component's fields.
//!
//! # The job, and only the job
//!
//! `docs/plan/08-editor.md`'s missing-pieces list asks for "a reflection-style
//! property hook per component", and UI rung 8 asked
//! for "a reflection-driven property inspector with per-type overrides". That is
//! the whole of what this crate is for, and it is a much smaller thing than
//! general-purpose reflection: a panel **shows a value, edits it, and reports
//! the change as an edit command**. So there is no type registry, no
//! construct-from-nothing, no dynamic call, and no serialisation — the scene
//! format already has one and this crate does not duplicate it (see below).
//!
//! Three pieces:
//!
//! * [`Reflect`] — what a value is made of, and how to read and write its
//!   leaves. Object-safe: an inspector holds `&mut dyn Reflect` and descends.
//! * [`derive@Reflect`] — the one annotation a component carries, with the
//!   per-field attributes a panel needs.
//! * [`get_path`] and [`set_path`] — one leaf by name, which is the form an
//!   **undoable** edit takes: a command records the path and the value it
//!   replaced, and its inverse is the same call with the old value.
//!
//! ```
//! use crcbl_reflect::{Kind, Reflect, Value, get_path, set_path};
//!
//! /// One brick of a breakout board, as `apps/breakout` spells it.
//! #[derive(Reflect)]
//! struct Brick {
//!     position: [f64; 3],
//!     #[reflect(name = "Half extents", min = 0.0, max = 32.0, step = 0.05)]
//!     half_extents: [f64; 3],
//! }
//!
//! let mut brick = Brick {
//!     position: [1.0, 2.0, 3.0],
//!     half_extents: [0.5, 0.25, 0.5],
//! };
//!
//! // The panel draws a row per field, labelled as the attribute says.
//! assert_eq!(brick.kind(), Kind::Struct);
//! assert_eq!(brick.fields()[1].label, "Half extents");
//! assert_eq!(brick.fields()[1].step, Some(0.05));
//!
//! // An edit reads the old value, writes the new one, and can put it back.
//! let before = get_path(&brick, "position.1").unwrap();
//! set_path(&mut brick, "position.1", &Value::Float(9.0)).unwrap();
//! assert_eq!(brick.position, [1.0, 9.0, 3.0]);
//! set_path(&mut brick, "position.1", &before).unwrap();
//! assert_eq!(brick.position, [1.0, 2.0, 3.0]);
//! ```
//!
//! # The derive, and its attributes
//!
//! [`derive@Reflect`] documents the whole vocabulary; these are the examples,
//! which live here because a doctest compiles in the crate that defines the item
//! and the macro's own crate cannot name this one.
//!
//! ```
//! use crcbl_reflect::{Kind, Range, Reflect};
//!
//! #[derive(Reflect)]
//! struct Sun {
//!     #[reflect(name = "Elevation", min = -1.0, max = 1.0, step = 0.01)]
//!     elevation: f32,
//!     color: [f32; 3],
//!     #[reflect(skip)]
//!     cached_direction: [f32; 3],
//! }
//!
//! let sun = Sun {
//!     elevation: 0.5,
//!     color: [1.0, 0.9, 0.8],
//!     cached_direction: [0.0, 1.0, 0.0],
//! };
//!
//! assert_eq!(sun.kind(), Kind::Struct);
//! assert_eq!(sun.fields().len(), 2, "the skipped field has no row");
//! assert_eq!(sun.fields()[0].label, "Elevation");
//! assert_eq!(sun.fields()[0].range, Some(Range { min: -1.0, max: 1.0 }));
//! ```
//!
//! An enum describes its **active** variant, and nothing else:
//!
//! ```
//! use crcbl_reflect::{Kind, Reflect, Value, get_path};
//!
//! #[derive(Reflect)]
//! enum Shape {
//!     Platform { width: f64, depth: f64 },
//!     Dome { radius: f64 },
//! }
//!
//! let shape = Shape::Dome { radius: 6.0 };
//! assert_eq!(shape.kind(), Kind::Enum);
//! assert_eq!(shape.variant(), Some("Dome"));
//! assert_eq!(get_path(&shape, "radius"), Ok(Value::Float(6.0)));
//! assert!(get_path(&shape, "width").is_err(), "that is the other variant's");
//! ```
//!
//! A field whose type is not [`Reflect`] is refused **where it is written**,
//! rather than quietly left out of the panel:
//!
//! ```compile_fail,E0277
//! use crcbl_reflect::Reflect;
//!
//! struct NotReflected;
//!
//! #[derive(Reflect)]
//! struct Component {
//!     opaque: NotReflected,
//! }
//! ```
//!
//! # Why this is not the scene codecs over again
//!
//! `crcbl_scene::scn` already writes a component to a file and reads it back,
//! bounded by `serde`. It is not what a panel needs, for two reasons that hold
//! whatever the format is:
//!
//! * **`Serialize` describes a whole value, not a field.** A panel edits **one**
//!   field and leaves the rest alone; serde's write path produces a document,
//!   and getting a single field back in would mean parsing the document,
//!   replacing a node and re-parsing — an edit whose failure mode is the whole
//!   component, not the one row.
//! * **A file carries no editing metadata.** A display name, a slider's range
//!   and a drag's step are things the *panel* needs and the *format* must not
//!   have: they would be written into every chunk file and read by nothing.
//!   `#[serde(skip)]` and `#[reflect(skip)]` are also different questions — a
//!   field can be derived and not persisted, or persisted and not shown.
//!
//! The two are complementary and the arrow between them is deliberately absent:
//! nothing here depends on `crcbl-scene`, and `crcbl-scene` does not depend on
//! this.
//!
//! # What this crate deliberately does not do
//!
//! * **No UI.** UI rung 8's inspector widget is
//!   `crcbl_ui::tree::widgets::inspector`, and the arrow points **from** it to
//!   here: this crate does not depend on `crcbl-ui` and never will, because a
//!   component has to be able to describe itself without dragging a stylesheet,
//!   a glyph atlas and a layout engine behind it. (Until 2026-09-16 there was no
//!   arrow in either direction; rung 8 added the one that exists.)
//! * **No variant switching.** [`Reflect::variant`] names an enum's active
//!   variant and [`Reflect::fields`] describes that variant's fields; there is
//!   no way to *change* which variant is active. Doing so means constructing the
//!   new variant, which needs a default for every field it has, which is a
//!   second mechanism nothing has asked for yet.
//! * **No collections that change shape.** `Vec<T>`, `Option<T>` and the maps
//!   are absent for the same reason; `crates/crcbl-reflect/src/impls.rs` says
//!   what is covered and what is not.
//! * **No construction.** Every entry point takes a value that already exists.

use core::any::Any;

mod impls;
mod path;
mod value;

pub use crcbl_reflect_derive::Reflect;
pub use path::{PathError, get_path, set_path};
pub use value::{Field, Kind, Range, SetError, Value, ValueKind};

/// What an inspector needs from a value: what it is made of, and how to read and
/// write the leaves inside it.
///
/// # Object-safe, on purpose
///
/// A panel descends a component it does not know the type of, so every method
/// here takes `&self` or `&mut self`, is free of generics and of `Self` by
/// value, and returns `&dyn Reflect` rather than an associated type.
/// `&mut dyn Reflect` is the working currency of this crate: [`set_path`] takes
/// one, [`field_mut`](Self::field_mut) returns one, and neither could exist
/// otherwise.
///
/// [`Any`] is a supertrait so that **per-type overrides** are possible — rung
/// 8's own words. A panel that wants to draw a `DVec3` as one XYZ widget rather
/// than three rows recognises it with [`as_any`](Self::as_any), which
/// `&dyn Reflect` alone cannot do.
///
/// # Implementing it by hand
///
/// Use [`derive@Reflect`]. A hand-written impl must keep two things in step, and
/// nothing checks them for you: [`fields`](Self::fields) and
/// [`field`](Self::field) describe the same children **in the same order**, and
/// [`kind`](Self::kind) agrees with what [`get`](Self::get) returns — a
/// [`Kind::Leaf`] has a value and a composite does not.
pub trait Reflect: Any {
    /// The type's name, as a panel labels the row's type.
    ///
    /// The short name for a derived type (`"Brick"`, not the module path), and
    /// the Rust spelling for the built-ins (`"f64"`, `"[f64; 3]"`).
    fn type_name(&self) -> &'static str;

    /// What this value is made of, which decides how a panel draws it.
    fn kind(&self) -> Kind;

    /// This leaf's value, or `None` when [`kind`](Self::kind) is not
    /// [`Kind::Leaf`].
    fn get(&self) -> Option<Value>;

    /// Writes `value` into this leaf.
    ///
    /// A refused write leaves the value untouched.
    ///
    /// # Errors
    ///
    /// [`SetError`]: a value of the wrong kind, one that does not fit, a
    /// non-finite float, or a target that is not a leaf at all.
    fn set(&mut self, value: &Value) -> Result<(), SetError>;

    /// One [`Field`] per row a panel draws for this value.
    ///
    /// A [`Kind::Struct`]'s fields; a [`Kind::Enum`]'s **active variant's**
    /// fields; empty for a [`Kind::Leaf`], which has no rows, and for a
    /// [`Kind::List`], whose elements are labelled by their index instead.
    ///
    /// A field carrying `#[reflect(skip)]` is not here and is not reachable
    /// through [`field`](Self::field) either — the two agree by construction.
    fn fields(&self) -> &'static [Field];

    /// The `index`th child: the field [`fields`](Self::fields) describes at that
    /// position, or the list element at that index.
    fn field(&self, index: usize) -> Option<&dyn Reflect>;

    /// [`field`](Self::field), for a write.
    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect>;

    /// The active variant's name, for a [`Kind::Enum`]; `None` for everything
    /// else.
    fn variant(&self) -> Option<&'static str>;

    /// This value as [`Any`], so a panel can recognise a type it has an override
    /// for.
    fn as_any(&self) -> &dyn Any;

    /// [`as_any`](Self::as_any), for a write.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
