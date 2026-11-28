//! `#[derive(Error)]` — the one derive every error type in this workspace uses.
//!
//! It writes `Display` from each variant's `#[error("…")]` message,
//! `std::error::Error` with the `source()` its fields name, and `From` for a
//! `#[from]` field. The attribute syntax is `thiserror`'s, so a crate moves over
//! by changing the derive's path (`thiserror::Error` → `crcbl_error::Error`) and
//! its manifest line, and every message reads byte for byte as it did.
//!
//! This crate is the whole of it: a `proc-macro = true` crate exports nothing
//! but macros, and the expansion names nothing outside `core`, so there is no
//! runtime half to depend on beside it.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod attrs;
mod expand;
mod fmt;

/// Implements `Display`, `std::error::Error` and, for a `#[from]` field,
/// `From` for a struct or enum.
///
/// ```
/// use std::path::PathBuf;
///
/// #[derive(Debug, crcbl_error::Error)]
/// enum LoadError {
///     #[error("cannot read {path}")]
///     Read {
///         path: PathBuf,
///         source: std::io::Error,
///     },
///     #[error("{0} is {1:#x} bytes, over the limit")]
///     TooLarge(String, u64),
///     #[error(transparent)]
///     Parse(#[from] std::num::ParseIntError),
/// }
///
/// let error = LoadError::TooLarge("level.scn".into(), 255);
/// assert_eq!(error.to_string(), "level.scn is 0xff bytes, over the limit");
/// let error: LoadError = "x".parse::<u8>().unwrap_err().into();
/// assert_eq!(error.to_string(), "invalid digit found in string");
/// ```
///
/// # Messages
///
/// `#[error("…", args…)]` goes on a struct, or on every variant of an enum,
/// and is the body of a `format_args!` run with the fields bound as locals:
///
/// * `{field}` names a field, and `{0}`, `{1}`, … a tuple field. Any format
///   spec works on either (`{field:?}`, `{0:#x}`, `{len:02x}`).
/// * Extra arguments follow the string, and in them `.field` and `.0` are the
///   field (`#[error("{}", .0.display())]`). A `name = value` argument shadows
///   a field of the same name in the string.
/// * A `Path` or `PathBuf` field written with no spec at all (`{path}`, not
///   `{path:?}`) is shown through `Path::display`. The test is on the type as
///   written, so an alias of `PathBuf` is not seen through and fails to compile
///   as not `Display`.
///
/// `#[error(transparent)]` takes a variant (or struct) with exactly one field
/// and forwards both `Display` and `source()` to it, so the wrapper is
/// invisible in an error chain.
///
/// # Sources and `From`
///
/// `source()` returns the field marked `#[source]` or `#[from]`, or, when no
/// field is marked, a named field called `source`; it returns `None` for a
/// variant with neither. A source field's type must implement
/// `std::error::Error + 'static`.
///
/// `#[from]` also writes `From<FieldType>` for the type, and so must be on the
/// variant's only field.
///
/// # Generics
///
/// A generic type gets impls over the generics and `where` clause it declares
/// and no added bounds, so a bound a message or a source needs (`T: Display`,
/// `E: Error + 'static`) is written where the type is declared.
///
/// # What it refuses
///
/// The parts of thiserror's surface this workspace does not use are refused,
/// each with an error at the attribute or field, rather than expanded into
/// something else:
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[error("an enum-level message")]
/// enum E {
///     #[error("a")]
///     A,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[error(fmt = core::fmt::Debug::fmt)]
/// struct E;
/// ```
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[error("with a backtrace")]
/// struct E(std::backtrace::Backtrace);
/// ```
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[error("optional")]
/// struct E {
///     #[source]
///     inner: Option<std::io::Error>,
/// }
/// ```
///
/// And the combinations that mean nothing. A variant with no message:
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// enum E {
///     A,
/// }
/// ```
///
/// `#[from]` beside another field, which `From` would have nothing to fill
/// with:
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// enum E {
///     #[error("io")]
///     Io {
///         #[from]
///         inner: std::io::Error,
///         path: String,
///     },
/// }
/// ```
///
/// Two sources:
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[error("two")]
/// struct E {
///     #[source]
///     a: std::io::Error,
///     #[source]
///     b: std::io::Error,
/// }
/// ```
///
/// `transparent` over anything but one field, or with `#[source]` on it:
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// enum E {
///     #[error(transparent)]
///     Two(std::io::Error, std::io::Error),
/// }
/// ```
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// enum E {
///     #[error(transparent)]
///     Io(#[source] std::io::Error),
/// }
/// ```
///
/// `#[from]` or `#[source]` anywhere but a field, and `#[from]` with
/// arguments:
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[source]
/// #[error("on the type")]
/// struct E(std::io::Error);
/// ```
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[error("arguments")]
/// struct E(#[from(std::io::Error)] std::io::Error);
/// ```
///
/// A `{0}` beside an unnamed extra argument, which could mean either:
///
/// ```compile_fail
/// #[derive(Debug, crcbl_error::Error)]
/// #[error("{0} {}", 1)]
/// struct E(u8);
/// ```
///
/// And a union:
///
/// ```compile_fail
/// #[derive(crcbl_error::Error)]
/// #[error("union")]
/// union E {
///     a: u8,
/// }
/// ```
#[proc_macro_derive(Error, attributes(error, from, source))]
pub fn derive_error(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand::derive(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
