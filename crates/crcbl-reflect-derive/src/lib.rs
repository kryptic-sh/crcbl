//! `#[derive(Reflect)]` — the one annotation a component carries so an editor's
//! property panel can show and edit its fields.
//!
//! This crate is the macro half of `crcbl-reflect` and has no API of its own: it
//! is re-exported from there, and `crcbl_reflect::Reflect` is both the trait and
//! this derive. Nothing should name this crate directly.
//!
//! **The worked examples are in `crates/crcbl-reflect/src/lib.rs`**, not here,
//! and that is a fact about doctests rather than about where the documentation
//! belongs: a doctest compiles in the crate that *defines* the item, this crate
//! is what `crcbl-reflect` depends on rather than the other way round, and an
//! example of the derive has to name the trait it implements.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod attrs;
mod expand;

/// Describes a type's fields for an editor's property panel.
///
/// # What it covers
///
/// * **Structs** with named fields, with unnamed ones (whose rows are named
///   `"0"`, `"1"`, … and reached by those names), and unit structs.
/// * **Enums**, as a `Kind::Enum` whose rows are the **active variant's**
///   fields. Unit, tuple and struct variants all work.
///
/// A **union** is refused: which field of one is live cannot be read off the
/// value, and reading the wrong one is unsafe. An enum with **no variants** is
/// refused too — no value of it can exist, so there is nothing to show.
///
/// A field whose type does not implement `Reflect` is a **compile error at that
/// field**, not a silently missing row: the expansion's coercion carries the
/// field's own span. Use `#[reflect(skip)]` to leave a field out on purpose.
///
/// A **generic** type gets an impl over the generics it declares and no extra
/// bounds, so `T: Reflect` has to be written where the type is declared — this
/// derive does not guess which parameters end up in a row.
///
/// # Attributes
///
/// On the type:
///
/// * `#[reflect(crate = "crcbl::reflect")]` — where to find `crcbl-reflect` from
///   the deriving crate. A game names the engine and nothing else, so it reaches
///   the trait through the umbrella; the default, `::crcbl_reflect`, is right
///   for a crate that depends on it directly. The same knob, for the same
///   reason, as `#[serde(crate = "crcbl::serde")]`.
///
/// On a field:
///
/// * `#[reflect(skip)]` — no row, and no path segment. It may not be combined
///   with any of the three below: a row that is not drawn cannot be labelled.
/// * `#[reflect(name = "Half extents")]` — what the row is labelled with. The
///   field's own name stays the path segment, so a label can be changed without
///   invalidating a recorded edit command.
/// * `#[reflect(min = …, max = …)]` — the interval a drag-value is clamped to.
///   The two are set together; half a range is refused, and so is a `max` below
///   its `min`.
/// * `#[reflect(step = …)]` — how far one notch moves the field.
///
/// The range and the step are **advisory**: they are what a widget bounds itself
/// by, and `Reflect::set` does not read them, because a path resolution reaches
/// the leaf without passing the row that carries them.
#[proc_macro_derive(Reflect, attributes(reflect))]
pub fn derive_reflect(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand::derive(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
