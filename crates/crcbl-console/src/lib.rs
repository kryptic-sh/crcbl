//! The console's registry: what a variable is, what a command is, where the
//! list of them comes from, and what a typed line does.
//!
//! Valve's Source engine is the model, on the user's instruction: a `ConVar`
//! **is** its own storage, so the code that owns a knob reads it directly and
//! never polls the console; a `ConCommand` is a name and a function; `help` and
//! `find` are how everything else is discovered. `docs/plan/52-debug-console.md`
//! is the design and the record of what was declined on the way.
//!
//! This crate depends on nothing but `core`/`std`. It draws nothing, reads no
//! settings file and names no engine type — a variable whose storage lives
//! elsewhere is a [`Binding`], written by the crate that owns the storage. So
//! the whole of it is testable headless and compiles on every target the engine
//! ships to, WebAssembly included.
//!
//! ```
//! use crcbl_console::{Context, Flags, Registry, table};
//!
//! crcbl_console::convar! {
//!     /// Draw the ambient-occlusion channel as grey instead of the shaded frame.
//!     pub static r_ao_view: bool = false;
//! }
//! crcbl_console::convar! {
//!     /// Which edge-antialiasing pass runs.
//!     #[flags(ARCHIVE)]
//!     pub static antialiasing: &'static str one_of ["none", "fxaa", "smaa"] = "none";
//! }
//!
//! // One list per crate, gathered at one seam.
//! let registry = Registry::gather(&[table![r_ao_view, antialiasing]])
//!     .expect("no two entries claim one name");
//!
//! let mut host = ();
//! let mut cx = Context::new(&registry, &mut host);
//! registry.execute(&mut cx, "antialiasing = smaa").expect("a value in the set");
//! assert_eq!(cx.lines(), ["antialiasing = smaa"]);
//!
//! // And the code that owns the knob reads it without a lookup.
//! assert_eq!(antialiasing.get_enum(), "smaa");
//! assert_eq!(antialiasing.flags(), Flags::ARCHIVE);
//! ```

mod builtin;
mod command;
mod complete;
pub mod guard;
mod history;
mod macros;
mod parse;
mod registry;
mod table;
mod value;
mod var;

pub use builtin::builtin_table;
pub use command::{ConCommand, Context};
pub use complete::Completion;
pub use history::{HISTORY_LINES, History};
pub use parse::{Line, Statement, parse_line};
pub use registry::{Duplicate, Entry, Registry};
pub use table::Table;
pub use value::{Fault, Kind, Value};
pub use var::{Binding, ConVar, Flags, Var};

/// Trims a declaration's doc comment into the help line.
///
/// Not part of the API: [`convar!`] and [`concommand!`] call it on the
/// concatenated `#[doc]` literals, each of which carries the space that follows
/// the `///`. Public only because a `macro_rules!` expands in the caller's
/// crate — the same reason `crcbl_core::log`'s helpers are public.
#[doc(hidden)]
#[must_use]
pub const fn __help(help: &'static str) -> &'static str {
    help.trim_ascii()
}
