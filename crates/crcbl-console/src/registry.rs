//! The gathered table: what the console knows, and what running a line does.

use std::cmp::Ordering;
use std::fmt;

use crate::command::{ConCommand, Context};
use crate::parse::{Statement, parse_line};
use crate::table::Table;
use crate::value::{Fault, Value};
use crate::var::Var;

/// One thing the console knows about.
#[derive(Clone, Copy, Debug)]
pub enum Entry {
    /// A variable, of either storage shape.
    Var(Var),
    /// A command.
    Command(&'static ConCommand),
}

impl Entry {
    /// The name a person types.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Var(var) => var.name(),
            Self::Command(command) => command.name(),
        }
    }

    /// The one-line help.
    #[must_use]
    pub const fn help(self) -> &'static str {
        match self {
            Self::Var(var) => var.help(),
            Self::Command(command) => command.help(),
        }
    }
}

/// Two entries claiming one name.
///
/// Refused at [`gather`](Registry::gather) rather than resolved, because either
/// resolution is wrong: the console would set one crate's variable and the other
/// crate would go on reading its own.
#[derive(Clone, Debug)]
pub struct Duplicate {
    /// The name both claim, folded to lower case — which is how they collided,
    /// since [`Registry::lookup`] does not distinguish case.
    pub name: String,
    /// The entry gathered first, in table order.
    pub first: Entry,
    /// The entry that collided with it.
    pub second: Entry,
}

impl fmt::Display for Duplicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "two console entries claim the name `{}`: `{}` ({}) and `{}` ({})",
            self.name,
            self.first.name(),
            self.first.help(),
            self.second.name(),
            self.second.help()
        )
    }
}

impl std::error::Error for Duplicate {}

/// Every variable and command the console can reach, sorted by name.
///
/// Built once — at `Loop::new`, in the engine — out of the per-crate
/// [`Table`]s, plus the built-ins this crate always carries.
#[derive(Debug)]
pub struct Registry {
    vars: Vec<Var>,
    commands: Vec<&'static ConCommand>,
    entries: Vec<Entry>,
}

impl Registry {
    /// Gather `tables` into one registry.
    ///
    /// The built-in table is added here rather than by the caller, so `help`,
    /// `find`, `echo` and `clear` exist in every registry and a host cannot ship
    /// a console without them. Do not pass
    /// [`builtin_table`](crate::builtin_table) as well — that is a duplicate,
    /// and is refused as one.
    ///
    /// Everything is sorted by name, without regard to case, and ties are
    /// refused.
    ///
    /// # Errors
    ///
    /// A [`Duplicate`] naming both entries, the first time two claim one name.
    pub fn gather(tables: &[Table]) -> Result<Self, Duplicate> {
        let mut vars: Vec<Var> = Vec::new();
        let mut commands: Vec<&'static ConCommand> = Vec::new();
        let builtin = crate::builtin_table();
        for table in tables.iter().chain(std::iter::once(&builtin)) {
            vars.extend(table.vars().iter().map(|var| Var::Static(var)));
            vars.extend(table.bindings().iter().map(|bound| Var::Bound(bound)));
            commands.extend(table.commands().iter().copied());
        }

        // A stable sort, so "first" in a duplicate is the entry the earlier
        // table declared.
        vars.sort_by(|a, b| cmp_names(a.name(), b.name()));
        commands.sort_by(|a, b| cmp_names(a.name(), b.name()));

        let mut entries: Vec<Entry> = vars
            .iter()
            .copied()
            .map(Entry::Var)
            .chain(commands.iter().copied().map(Entry::Command))
            .collect();
        entries.sort_by(|a, b| cmp_names(a.name(), b.name()));

        for pair in entries.windows(2) {
            let [first, second] = pair else {
                unreachable!("`windows(2)` yields pairs")
            };
            if cmp_names(first.name(), second.name()) == Ordering::Equal {
                return Err(Duplicate {
                    name: first.name().to_ascii_lowercase(),
                    first: *first,
                    second: *second,
                });
            }
        }

        Ok(Self {
            vars,
            commands,
            entries,
        })
    }

    /// Every variable, sorted by name.
    #[must_use]
    pub fn vars(&self) -> &[Var] {
        &self.vars
    }

    /// Every command, sorted by name.
    #[must_use]
    pub fn commands(&self) -> &[&'static ConCommand] {
        &self.commands
    }

    /// Every entry, variables and commands together, sorted by name.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry called `name`, matched without regard to case.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<Entry> {
        self.entries
            .binary_search_by(|entry| cmp_names(entry.name(), name))
            .ok()
            .map(|at| self.entries[at])
    }

    /// Every entry whose name or help holds `needle`, without regard to case.
    ///
    /// Source's `find`, and the way a variable is discovered without knowing its
    /// prefix — which is why the help is searched too.
    #[must_use]
    pub fn find(&self, needle: &str) -> Vec<Entry> {
        let needle = needle.to_ascii_lowercase();
        self.entries
            .iter()
            .copied()
            .filter(|entry| {
                entry.name().to_ascii_lowercase().contains(&needle)
                    || entry.help().to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }

    /// Run a typed line: every statement its `;` separated, in order.
    ///
    /// A bare variable prints; a variable with a value is coerced through its
    /// [`Kind`](crate::Kind) and set; a command runs. The first statement to
    /// fault stops the line, because the ones after it were typed on the
    /// assumption it worked.
    ///
    /// # Errors
    ///
    /// A [`Fault`]: a line that will not parse, a name nothing knows, a value
    /// the kind refuses, or whatever a command refused.
    pub fn execute(&self, cx: &mut Context<'_>, line: &str) -> Result<(), Fault> {
        for statement in parse_line(line)?.statements() {
            self.run_statement(cx, statement)?;
        }
        Ok(())
    }

    /// One statement of a line.
    fn run_statement(&self, cx: &mut Context<'_>, statement: &Statement) -> Result<(), Fault> {
        let name = &statement.name;
        let Some(entry) = self.lookup(name) else {
            return Err(Fault::new(format!(
                "unknown command or variable `{name}` — try `find {name}`"
            )));
        };
        match entry {
            Entry::Command(command) => {
                let args: Vec<&str> = statement.args.iter().map(String::as_str).collect();
                command.run(cx, &args)
            }
            Entry::Var(var) => {
                if statement.args.is_empty() {
                    let value = var.get(cx.host());
                    let line = describe(var, &value);
                    cx.print(line);
                    return Ok(());
                }
                // Joined rather than taken one at a time, so a multi-word enum
                // value reads the way the plan writes it: `debug_view ambient
                // occlusion`.
                let typed = statement.args.join(" ");
                let value = var.kind().parse(&typed)?;
                var.set(cx.host_mut(), &value)?;
                let now = var.get(cx.host());
                cx.print(format!("{} = {now}", var.name()));
                Ok(())
            }
        }
    }
}

/// One line describing a variable: its value, its default, its flags, its help.
///
/// Shared by the bare-variable print and by `help`, so the two cannot show a
/// variable differently.
pub(crate) fn describe(var: Var, value: &Value) -> String {
    let default = match var.default() {
        Some(default) => format!(" (default: {default})"),
        None => String::new(),
    };
    let flags = if var.flags().is_empty() {
        String::new()
    } else {
        format!(" [{}]", var.flags())
    };
    format!("{} = {value}{default}{flags} — {}", var.name(), var.help())
}

/// Order two names the way the console does: ASCII case-folded, so `R_AO_VIEW`
/// and `r_ao_view` are one name and not two.
pub(crate) fn cmp_names(a: &str, b: &str) -> Ordering {
    a.bytes()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
}
