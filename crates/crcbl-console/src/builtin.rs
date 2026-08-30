//! The commands every registry carries.
//!
//! Four of plan 52's table, and the four that need nothing but the registry
//! itself: `help`, `find`, `echo` and `clear`. The rest — `toggle`, `reset`,
//! `save`, `dump`, `log`, `debug_view`, `pause`, `quit`, `fps` — belong to the
//! crates that own the behaviour and arrive with them, which is decision 2
//! applied to commands.
//!
//! They are declared here with [`concommand!`](crate::concommand) and listed in
//! [`builtin_table`], exactly as another crate would declare and list its own —
//! so this crate's own guard test has something real to hold to its table.

use std::any::Any;

use crate::registry::{Entry, describe};
use crate::table::Table;
use crate::value::Fault;

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

/// The table this crate contributes to every registry.
///
/// [`Registry::gather`](crate::Registry::gather) adds it itself, so a caller
/// never passes it — passing it too would be a duplicate, and is refused as one.
#[must_use]
pub fn builtin_table() -> Table {
    crate::table![cmd help, cmd find, cmd echo, cmd clear]
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
