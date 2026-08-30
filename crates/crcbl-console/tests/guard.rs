//! This crate holding its own table to its own source.
//!
//! Plan decision 2's guard, applied here first because the built-in commands are
//! themselves an instance of it: they are declared beside the behaviour and
//! listed once, exactly as `crcbl-render`'s will be. Every other crate that owns
//! a console entry writes this same test over its own `src/` and its own
//! `console_table()`.

use std::collections::BTreeSet;

use crcbl_console::{builtin_table, guard};

#[test]
fn every_convar_in_this_crates_source_is_in_its_table() {
    let declared = guard::declared_names("src").expect("this crate's src directory");
    // A scan that matched nothing would pass this test for every table there
    // is, so the scan itself has to be shown to have found something.
    assert!(
        !declared.is_empty(),
        "the scan found no declarations at all, so it proves nothing about the table"
    );

    let table = builtin_table();
    let listed: BTreeSet<&str> = table
        .vars()
        .iter()
        .map(|var| var.name())
        .chain(table.bindings().iter().map(|bound| bound.name()))
        .chain(table.commands().iter().map(|command| command.name()))
        .collect();

    for name in &declared {
        assert!(
            listed.contains(name.as_str()),
            "`{name}` is declared in this crate's source and missing from its table: {listed:?}"
        );
    }
}

#[test]
fn the_guard_reads_the_names_the_declarations_actually_use() {
    // The other half of the guard: that the scan finds the built-ins by the
    // names the console answers to, not by some other ident in the file.
    let declared = guard::declared_names("src").expect("this crate's src directory");
    assert_eq!(declared, ["clear", "echo", "find", "help"]);
}
