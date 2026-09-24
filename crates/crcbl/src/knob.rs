//! Driving a console variable by name: the half of a debug fixture that is not
//! about what the variable *means*.
//!
//! ```text
//!  key / pause row ──▶ Knob::named(table, "r_ssao_radius") ──▶ the ConVar
//!  console line ─────────────────────────────────────────────▶ ┘
//!                                       (the variable IS the storage)
//! ```
//!
//! # Why a sample reaches its knobs by name
//!
//! A crate declares its variables beside the code that owns them and lists them
//! once, in a `console_table()` of its own — debug-console decision 2 in
//! `docs/notes/tooling.md`. Some of those modules are private, so a name is the
//! only handle a sample has; and where the module is public, the name is still
//! **the seam a person typing `r_ssao_radius 1.5` goes through**, so a pause
//! row and a typed line cannot hold two answers that disagree.
//!
//! # Ranges and sets are the variable's, not the caller's
//!
//! [`Knob::cycle`] walks the names the variable itself declares and
//! [`Knob::set_float`] clamps into the range it itself declares, so a sample
//! never writes a bound or a member list down a second time — a copy that goes
//! stale the day the declaration moves. [`Knob::reset`] is the same rule for the
//! shipped value.
//!
//! # Nothing here is kept
//!
//! A [`Knob`] is a borrowed pointer at a cell the console owns. It holds no
//! state, and two knobs named the same twice are the same cell.

use crcbl_console::{ConVar, Kind, Table, Value};

use crate::log;

/// One console variable, found by name and driven generically.
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    var: &'static ConVar,
}

impl Knob {
    /// The variable `name` names, out of `table`.
    ///
    /// The table is an argument because this crate is not the only one with a
    /// console table: `crcbl::console_table`, [`crate::render::console_table`]
    /// and [`crate::core::console_table`] are three lists, and which one a
    /// sample's knobs live in is the sample's business.
    ///
    /// # Panics
    ///
    /// If the table declares no such variable, which is a mistake in the
    /// caller's list of names rather than a condition a run can be in — a
    /// sample's `every_knob_this_sample_drives_is_declared_by_the_engine` is
    /// what catches it with no GPU and no window.
    #[must_use]
    pub fn named(table: Table, name: &str) -> Self {
        let var = table
            .vars()
            .iter()
            .copied()
            .find(|var| var.name() == name)
            .unwrap_or_else(|| panic!("the console table declares no `{name}` variable"));
        Self { var }
    }

    /// The variable itself, for the readings a caller takes off it.
    #[must_use]
    pub const fn var(self) -> &'static ConVar {
        self.var
    }

    /// Writes `value`, and says so in the log if the console refused.
    ///
    /// **Refusal is reported rather than dropped.** Every setter here clamps or
    /// chooses inside the variable's own [`Kind`] first, so a refusal means the
    /// two disagree — which is worth a line rather than a knob that silently
    /// did not move.
    pub fn set(self, value: &Value) {
        if let Err(fault) = self.var.set(value) {
            log::error!("the console refused {} = {value}: {fault}", self.var.name());
        }
    }

    /// The names a [`Kind::Enum`] variable accepts, in the order it declares
    /// them. Empty for any other kind.
    #[must_use]
    pub fn names(self) -> &'static [&'static str] {
        match self.var.kind() {
            Kind::Enum(names) => names,
            _ => &[],
        }
    }

    /// Moves an enum variable on to the next name it declares, wrapping.
    ///
    /// A no-op for a variable that is not an enum, which has no next name.
    pub fn cycle(self) {
        let names = self.names();
        if names.is_empty() {
            return;
        }
        let current = self.var.get_enum();
        let at = names
            .iter()
            .position(|entry| *entry == current)
            .unwrap_or(0);
        self.set(&Value::Enum(names[(at + 1) % names.len()]));
    }

    /// The inclusive float range the variable accepts, or `None` for another
    /// kind.
    #[must_use]
    pub fn range(self) -> Option<(f32, f32)> {
        match self.var.kind() {
            Kind::Float { min, max } => Some((min, max)),
            _ => None,
        }
    }

    /// Writes a float, **clamped into the variable's own range**, and answers
    /// with what it holds afterwards.
    ///
    /// Clamped rather than refused: the console rejects a value outside the
    /// declared range outright, so a key at the end of its travel or a page
    /// sending a number from outside the range would otherwise leave the cell
    /// where it was and read as a control wired to nothing.
    ///
    /// # Panics
    ///
    /// If the variable is not a float. [`ConVar::get_f32`] refuses the wrong
    /// kind the same way and for the same reason — a caller writing a float
    /// into a switch has a mistake in its list of names, not a value out of
    /// range — and [`Knob::range`] is how a caller that does not know asks
    /// first.
    pub fn set_float(self, value: f32) -> f32 {
        let (min, max) = self
            .range()
            .unwrap_or_else(|| panic!("console variable `{}` is not a float", self.var.name()));
        self.set(&Value::Float(value.clamp(min, max)));
        self.var.get_f32()
    }

    /// Puts the variable back to the value its own declaration gives.
    pub fn reset(self) {
        let default = self.var.default().clone();
        self.set(&default);
    }
}

/// Puts every knob in `names` back to the value the engine declares.
///
/// What a golden run needs between two frames it means to compare, and what a
/// binary does on its way out so a console left mid-experiment does not reach
/// the next process through a settings file.
pub fn reset_all(table: Table, names: &[&str]) {
    for name in names {
        Knob::named(table, name).reset();
    }
}
