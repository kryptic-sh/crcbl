//! The simulation's half of a [`Flags::SIM`] variable: a checked set waiting
//! for a tick boundary, and the store a simulation keeps the values in.
//!
//! A `SIM` variable is declared like any other — a [`ConVar`] beside the code
//! that reads it — but a typed set does not write it. The console checks the
//! line with [`Registry::sim_set`], hands the [`SimSet`] to its host through
//! [`Context::request_sim_set`](crate::Context::request_sim_set), and the host
//! carries it to whichever simulation it runs: its own, offline, or a server's
//! over the transport. That simulation applies it to its [`SimVars`] at the
//! start of its next tick, and its tick code reads the value from there.
//!
//! **The value is per simulation, not per process.** A listen host runs the
//! server's world and its own client in one process, and a test runs many
//! worlds at once; one atomic in a `static` would be shared by all of them, so
//! the [`ConVar`]'s cell keeps its default and is never read for the value.
//!
//! The value travels as the text the console prints it as and is parsed back
//! through the same [`Kind::parse`](crate::Kind::parse), so a server reads
//! exactly the value the client's console checked: an `f32` prints as the
//! shortest text that parses back to the same bits.

use std::fmt;

use crate::registry::{Entry, Registry, cmp_names};
use crate::value::{Fault, Value};
use crate::var::{ConVar, Flags, Var};

/// One checked set of a [`Flags::SIM`] variable: its name and a value its kind
/// admits, waiting for a tick boundary.
///
/// Built only by [`Registry::sim_set`], so the name is the registry's own
/// spelling and the value is in range.
#[derive(Clone, Debug, PartialEq)]
pub struct SimSet {
    name: &'static str,
    value: Value,
}

impl SimSet {
    /// The variable's name, as it was declared.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The value it is being set to.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    /// The value as the console prints it — the text a transport carries and
    /// [`Registry::sim_set`] parses back to the same value.
    #[must_use]
    pub fn value_text(&self) -> String {
        self.value.to_string()
    }
}

/// Prints the line that sets it: `sv_spin_rate 2`.
impl fmt::Display for SimSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.name, self.value)
    }
}

impl Registry {
    /// Check a set of the simulation variable `name` to the typed `text`.
    ///
    /// The boundary every `SIM` set goes through: the console on the line a
    /// person typed, and a server on the `(name, value)` text a client sent.
    /// `name` is matched without regard to case, as every lookup is.
    ///
    /// # Errors
    ///
    /// A [`Fault`] naming what was refused: a name nothing knows, a command, a
    /// variable that is not [`Flags::SIM`] or is [`Flags::READ_ONLY`], or a
    /// value its kind refuses — out of range included.
    pub fn sim_set(&self, name: &str, text: &str) -> Result<SimSet, Fault> {
        let var = match self.lookup(name) {
            Some(Entry::Var(var)) => var,
            Some(Entry::Command(command)) => {
                return Err(Fault::new(format!(
                    "`{}` is a command, not a variable",
                    command.name()
                )));
            }
            None => return Err(Fault::new(format!("unknown variable `{name}`"))),
        };
        if !var.flags().contains(Flags::SIM) {
            return Err(Fault::new(format!(
                "`{}` is not a simulation variable",
                var.name()
            )));
        }
        if var.flags().contains(Flags::READ_ONLY) {
            return Err(Fault::new(format!("`{}` is read-only", var.name())));
        }
        let value = var
            .kind()
            .parse(text)
            .map_err(|fault| Fault::new(format!("`{}`: {fault}", var.name())))?;
        Ok(SimSet {
            name: var.name(),
            value,
        })
    }
}

/// One simulation's values of every [`Flags::SIM`] variable in a registry.
///
/// Owned by the simulation — a server's host, or a game's offline world — and
/// written only by [`apply`](Self::apply), which that simulation calls at the
/// start of a tick. Starts with every variable at its declared default.
#[derive(Clone, Debug, Default)]
pub struct SimVars {
    /// Sorted by name, as the registry they came from is.
    values: Vec<(&'static ConVar, Value)>,
}

impl SimVars {
    /// Every [`Flags::SIM`] variable in `registry`, at its declared default.
    #[must_use]
    pub fn new(registry: &Registry) -> Self {
        let values = registry
            .vars()
            .iter()
            .filter_map(|var| match var {
                // `Binding::new` refuses the flag, so a static is the only
                // shape one can take.
                Var::Static(var) if var.flags().contains(Flags::SIM) => {
                    Some((*var, var.default().clone()))
                }
                Var::Static(_) | Var::Bound(_) => None,
            })
            .collect();
        Self { values }
    }

    /// The value of the simulation variable `name`, matched without regard to
    /// case, or `None` for a name this store does not hold.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(var, _)| cmp_names(var.name(), name).is_eq())
            .map(|(_, value)| value)
    }

    /// The value of `var`, a float simulation variable — what tick code reads
    /// instead of the [`ConVar`]'s own getter.
    ///
    /// # Panics
    ///
    /// When this store does not hold `var` or it is not a float. Either is a
    /// wiring mistake in the code that declared or gathered the variable, not
    /// something a person typed — the reasoning of [`ConVar::get_bool`].
    #[must_use]
    pub fn f32(&self, var: &ConVar) -> f32 {
        match self.get(var.name()) {
            Some(Value::Float(value)) => *value,
            Some(other) => panic!(
                "simulation variable `{}` is {}, not a float",
                var.name(),
                other.article_name()
            ),
            None => panic!(
                "simulation variable `{}` is not in this simulation's store",
                var.name()
            ),
        }
    }

    /// Write `set` — the tick boundary's one write.
    ///
    /// # Errors
    ///
    /// A [`Fault`] when this store does not hold the variable, or its kind
    /// refuses the value — possible only for a set checked against another
    /// registry than the one this store was built from.
    pub fn apply(&mut self, set: &SimSet) -> Result<(), Fault> {
        let Some((var, value)) = self
            .values
            .iter_mut()
            .find(|(var, _)| cmp_names(var.name(), set.name()).is_eq())
        else {
            return Err(Fault::new(format!(
                "`{}` is not a simulation variable of this simulation",
                set.name()
            )));
        };
        var.kind().check(var.name(), set.value())?;
        value.clone_from(set.value());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Built with the constructors rather than `convar!`, which
    // `guard::declared_names` would read as this crate's own entries.
    static T_RATE: ConVar = ConVar::new_float(
        "t_rate",
        "A rate the test simulation reads.",
        Flags::SIM,
        0.0,
        8.0,
        1.0,
    );
    static T_VIEW: ConVar =
        ConVar::new_bool("t_view", "A knob nothing simulates.", Flags::NONE, false);
    static T_GRAVITY: ConVar = ConVar::new_float(
        "t_gravity",
        "A simulation fact nobody may set.",
        Flags::SIM.union(Flags::READ_ONLY),
        0.0,
        20.0,
        9.8,
    );

    fn registry() -> Registry {
        static VARS: &[&ConVar] = &[&T_RATE, &T_VIEW, &T_GRAVITY];
        Registry::gather(&[crate::Table::new(VARS, &[], &[])]).expect("distinct names")
    }

    #[test]
    fn a_set_is_checked_against_the_kind_and_keeps_the_declared_spelling() {
        let set = registry().sim_set("T_RATE", "2.5").expect("in range");
        assert_eq!(set.name(), "t_rate");
        assert_eq!(set.value(), &Value::Float(2.5));
        assert_eq!(set.to_string(), "t_rate 2.5");
    }

    #[test]
    fn every_refusal_names_what_it_refused() {
        let registry = registry();
        let refusal = |name: &str, text: &str| {
            registry
                .sim_set(name, text)
                .expect_err("refused")
                .message()
                .to_owned()
        };
        assert_eq!(refusal("t_nope", "1"), "unknown variable `t_nope`");
        assert_eq!(refusal("help", "1"), "`help` is a command, not a variable");
        assert_eq!(
            refusal("t_view", "1"),
            "`t_view` is not a simulation variable"
        );
        assert_eq!(refusal("t_gravity", "1"), "`t_gravity` is read-only");
        assert_eq!(
            refusal("t_rate", "fast"),
            "`t_rate`: `fast` is not a number"
        );
        assert_eq!(refusal("t_rate", "9"), "`t_rate`: 9 is outside 0..=8");
    }

    #[test]
    fn a_value_round_trips_through_its_printed_text_bit_for_bit() {
        let registry = registry();
        // 0.1 has no exact binary form, so a text that dropped digits would
        // parse back to a neighbouring float.
        for typed in ["0.1", "1", "7.999999", "3.3333333"] {
            let sent = registry.sim_set("t_rate", typed).expect("in range");
            let received = registry
                .sim_set("t_rate", &sent.value_text())
                .expect("the printed text parses back");
            let (Value::Float(a), Value::Float(b)) = (sent.value(), received.value()) else {
                panic!("a float variable parses to a float");
            };
            assert_eq!(a.to_bits(), b.to_bits(), "{typed}");
        }
    }

    #[test]
    fn a_typed_set_is_requested_in_line_order_and_written_nowhere() {
        let registry = registry();
        let mut host = ();
        let mut cx = crate::Context::new(&registry, &mut host);
        registry
            .execute(&mut cx, "t_rate 2; t_rate 3")
            .expect("both in range");
        let requested: Vec<String> = cx.sim_sets().iter().map(SimSet::to_string).collect();
        assert_eq!(requested, ["t_rate 2", "t_rate 3"]);
        assert_eq!(
            cx.lines()[0],
            "t_rate 2 — sent to the simulation, which applies it at the start of its next tick"
        );
        assert_eq!(T_RATE.get(), Value::Float(1.0), "nothing was written");
        assert_eq!(cx.take_sim_sets().len(), 2);
        assert!(cx.sim_sets().is_empty(), "taken");
    }

    #[test]
    fn a_typed_set_the_kind_refuses_requests_nothing() {
        let registry = registry();
        let mut host = ();
        let mut cx = crate::Context::new(&registry, &mut host);
        let fault = registry
            .execute(&mut cx, "t_rate 99")
            .expect_err("out of range");
        assert_eq!(fault.message(), "`t_rate`: 99 is outside 0..=8");
        assert!(cx.sim_sets().is_empty());
    }

    #[test]
    fn a_bare_sim_variable_prints_its_default_and_flags_but_no_value() {
        let registry = registry();
        let mut host = ();
        let mut cx = crate::Context::new(&registry, &mut host);
        registry
            .execute(&mut cx, "t_rate")
            .expect("a bare variable prints");
        assert_eq!(
            cx.lines(),
            [
                "t_rate (default: 1) [SIM] — A rate the test simulation reads. (the simulation \
                 holds its value)"
            ]
        );
        assert!(cx.sim_sets().is_empty(), "printing requests nothing");
    }

    #[test]
    fn a_store_starts_at_the_defaults_and_holds_only_sim_variables() {
        let vars = SimVars::new(&registry());
        assert_eq!(vars.f32(&T_RATE), 1.0);
        assert_eq!(vars.get("t_gravity"), Some(&Value::Float(9.8)));
        assert_eq!(vars.get("t_view"), None);
    }

    #[test]
    fn applying_a_set_moves_the_store_and_not_the_static() {
        let registry = registry();
        let mut vars = SimVars::new(&registry);
        vars.apply(&registry.sim_set("t_rate", "4").expect("in range"))
            .expect("the store holds it");
        assert_eq!(vars.f32(&T_RATE), 4.0);
        assert_eq!(
            T_RATE.get(),
            Value::Float(1.0),
            "the static keeps its default"
        );
        assert_eq!(
            SimVars::new(&registry).f32(&T_RATE),
            1.0,
            "another simulation's store is its own"
        );
    }

    #[test]
    fn a_set_for_a_variable_the_store_lacks_is_refused() {
        let mut empty = SimVars::default();
        let set = registry().sim_set("t_rate", "2").expect("in range");
        assert_eq!(
            empty.apply(&set).expect_err("not held").message(),
            "`t_rate` is not a simulation variable of this simulation"
        );
    }

    #[test]
    #[should_panic(expected = "simulation variable `t_rate` is not in this simulation's store")]
    fn reading_a_variable_the_store_lacks_panics_naming_it() {
        let _ = SimVars::default().f32(&T_RATE);
    }
}
