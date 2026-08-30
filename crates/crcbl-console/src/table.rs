//! The one list a crate's console entries join.

use crate::command::ConCommand;
use crate::var::{Binding, ConVar};

/// Everything one crate exposes to the console.
///
/// A crate declares its variables and commands beside the code that owns them
/// and lists them **once**, in a `console_table()` of its own built with
/// [`table!`](crate::table!). The engine gathers those tables at one seam; a
/// declaration missing from its crate's list is a red test in that crate, which
/// is what [`guard::declared_names`](crate::guard::declared_names) is for.
///
/// Plan decision 2 records why this is a hand-written list rather than a
/// distributed slice: `linkme` does not list WebAssembly, and fifteen demos ship
/// as wasm.
#[derive(Clone, Copy, Debug)]
pub struct Table {
    vars: &'static [&'static ConVar],
    bindings: &'static [&'static Binding],
    commands: &'static [&'static ConCommand],
}

impl Table {
    /// A table with nothing in it — what a host that exposes nothing returns.
    pub const EMPTY: Self = Self::new(&[], &[], &[]);

    /// A table over three lists. [`table!`](crate::table!) writes this call.
    #[must_use]
    pub const fn new(
        vars: &'static [&'static ConVar],
        bindings: &'static [&'static Binding],
        commands: &'static [&'static ConCommand],
    ) -> Self {
        Self {
            vars,
            bindings,
            commands,
        }
    }

    /// The variables that are their own storage.
    #[must_use]
    pub const fn vars(&self) -> &'static [&'static ConVar] {
        self.vars
    }

    /// The variables whose storage is elsewhere.
    #[must_use]
    pub const fn bindings(&self) -> &'static [&'static Binding] {
        self.bindings
    }

    /// The commands.
    #[must_use]
    pub const fn commands(&self) -> &'static [&'static ConCommand] {
        self.commands
    }

    /// How many entries it holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.vars.len() + self.bindings.len() + self.commands.len()
    }

    /// Whether it holds nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
