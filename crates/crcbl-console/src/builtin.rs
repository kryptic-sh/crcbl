//! The commands every registry carries.
//!
//! Six of plan 52's table, and the six that need nothing but the registry
//! itself: `help`, `find`, `echo`, `clear`, `toggle` and `reset`. The rest —
//! `save`, `dump`, `log`, `debug_view`, `pause`, `quit`, `fps` — belong to the
//! crates that own the behaviour and arrive with them, which is decision 2
//! applied to commands.
//!
//! They are declared here with [`concommand!`](crate::concommand) and listed in
//! [`builtin_table`], exactly as another crate would declare and list its own —
//! so this crate's own guard test has something real to hold to its table.

use std::any::Any;

use crate::registry::{Entry, Registry, describe};
use crate::table::Table;
use crate::value::{Fault, Value};
use crate::var::{Flags, Var};

crate::concommand! {
    /// List every variable and command, or only those a prefix names.
    pub fn help(cx, args) {
        let registry = cx.registry();
        let prefix = args.first().copied().unwrap_or("").to_ascii_lowercase();
        for entry in registry.entries() {
            if !entry.name().to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }
            let line = line_for(*entry, cx.host());
            cx.print(line);
        }
        Ok(())
    }
}

crate::concommand! {
    /// Show every variable and command whose name or help holds the text.
    pub fn find(cx, args) {
        let needle = args.join(" ");
        if needle.is_empty() {
            return Err(Fault::new("find needs some text to look for"));
        }
        let registry = cx.registry();
        for entry in registry.find(&needle) {
            let line = line_for(entry, cx.host());
            cx.print(line);
        }
        Ok(())
    }
}

crate::concommand! {
    /// Print the arguments back as one line.
    pub fn echo(cx, args) {
        cx.print(args.join(" "));
        Ok(())
    }
}

crate::concommand! {
    /// Empty the console's view of the log. The log itself is untouched.
    pub fn clear(cx, _args) {
        cx.request_clear();
        Ok(())
    }
}

crate::concommand! {
    /// Flip a boolean variable to the other value.
    pub fn toggle(cx, args) {
        let [name] = args else {
            return Err(Fault::new("toggle takes the name of one boolean variable"));
        };
        let var = var_named(cx.registry(), name)?;
        let held = var.get(cx.host());
        // Borrowed rather than moved, so the refusal below can still name the
        // kind the variable actually is.
        let Value::Bool(held) = &held else {
            return Err(Fault::new(format!(
                "`{}` is {}, and toggle is for a bool",
                var.name(),
                held.article_name()
            )));
        };
        var.set(cx.host_mut(), &Value::Bool(!held))?;
        let now = var.get(cx.host());
        cx.print(format!("{} = {now}", var.name()));
        Ok(())
    }
}

crate::concommand! {
    /// Put a variable back to the value it was declared with; bare, every unsaved one.
    pub fn reset(cx, args) {
        match args {
            [name] => reset_one(cx, name),
            [] => reset_every(cx),
            _ => Err(Fault::new(
                "reset takes one variable name, or nothing at all",
            )),
        }
    }
}

/// `reset <name>`: one variable, back to its declared default.
fn reset_one(cx: &mut crate::Context<'_>, name: &str) -> Result<(), Fault> {
    let var = var_named(cx.registry(), name)?;
    let Some(default) = var.default() else {
        return Err(Fault::new(format!(
            "`{}` is stored through the settings stack, which keeps its own defaults",
            var.name()
        )));
    };
    var.set(cx.host_mut(), default)?;
    let now = var.get(cx.host());
    cx.print(format!("{} = {now}", var.name()));
    Ok(())
}

/// Bare `reset`: every variable that is neither saved nor read-only, back to
/// what it was declared holding.
///
/// **`ARCHIVE` is left alone deliberately** — plan decision 7's "every
/// non-`ARCHIVE` variable". A saved variable is the player's settings file, and
/// a debug command that emptied it would be a session's worth of preferences
/// gone; `READ_ONLY` is skipped because [`Var::set`] would refuse it, and a bare
/// `reset` that faulted half way through would leave the rest of the table
/// untouched with no way to tell which half moved.
///
/// Only the variables that actually differ are written, so what this prints is
/// what it changed.
fn reset_every(cx: &mut crate::Context<'_>) -> Result<(), Fault> {
    let registry = cx.registry();
    let mut moved = 0_usize;
    for var in registry.vars() {
        let flags = var.flags();
        if flags.contains(Flags::ARCHIVE) || flags.contains(Flags::READ_ONLY) {
            continue;
        }
        let Some(default) = var.default() else {
            continue;
        };
        if var.get(cx.host()) == *default {
            continue;
        }
        var.set(cx.host_mut(), default)?;
        let now = var.get(cx.host());
        cx.print(format!("{} = {now}", var.name()));
        moved += 1;
    }
    if moved == 0 {
        cx.print("every variable is already at its default");
    }
    Ok(())
}

/// The variable `name` names, for a command that has to have one.
///
/// A command of that name is refused by name rather than reported missing: it
/// exists, and telling someone `toggle help` is unknown would send them looking
/// for a typo that is not there.
fn var_named(registry: &Registry, name: &str) -> Result<Var, Fault> {
    match registry.lookup(name) {
        Some(Entry::Var(var)) => Ok(var),
        Some(Entry::Command(command)) => Err(Fault::new(format!(
            "`{}` is a command, not a variable",
            command.name()
        ))),
        None => Err(Fault::new(format!(
            "unknown variable `{name}` — try `find {name}`"
        ))),
    }
}

/// The table this crate contributes to every registry.
///
/// [`Registry::gather`](crate::Registry::gather) adds it itself, so a caller
/// never passes it — passing it too would be a duplicate, and is refused as one.
#[must_use]
pub fn builtin_table() -> Table {
    crate::table![cmd help, cmd find, cmd echo, cmd clear, cmd toggle, cmd reset]
}

/// One line for one entry, in the shape `help` and `find` both print.
fn line_for(entry: Entry, host: &dyn Any) -> String {
    match entry {
        Entry::Var(var) => {
            let value = var.get(host);
            describe(var, &value)
        }
        Entry::Command(command) => format!("{} — {}", command.name(), command.help()),
    }
}
