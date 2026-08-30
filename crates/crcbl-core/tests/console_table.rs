//! This crate's console table, held to this crate's source.
//!
//! Plan decision 2's second half, and the three lines
//! `crcbl_console::guard::declared_names` exists for: a declaration beside the
//! code it belongs to is only reachable once it is in the crate's one list, and
//! nothing but this makes the list keep up. Forgetting it is then a red test
//! rather than a command that quietly does not exist.

/// **Every `concommand!` and `convar!` in `crates/crcbl-core/src` is in
/// [`crcbl_core::console_table`].**
#[test]
fn every_console_declaration_in_this_crate_is_in_its_table() {
    let declared = crcbl_console::guard::declared_names("src").expect("this crate's src directory");
    // A scan that matched nothing would report success forever. `log` is this
    // crate's one command, so its absence means the walk did not reach `log.rs`
    // rather than that the crate declares nothing.
    assert!(
        declared.contains(&"log".to_owned()),
        "the scan did not reach `log.rs`: {declared:?}",
    );

    let table = crcbl_core::console_table();
    for name in &declared {
        assert!(
            table
                .commands()
                .iter()
                .any(|command| command.name() == name)
                || table.vars().iter().any(|var| var.name() == name),
            "`{name}` is declared in this crate's source and missing from its table",
        );
    }
}

/// The table is gatherable on its own, which is what the engine will do with it
/// at `Loop::new` — a name colliding with one of the built-ins would be refused
/// there and nowhere else.
#[test]
fn the_table_gathers_beside_the_built_ins() {
    let registry = crcbl_console::Registry::gather(&[crcbl_core::console_table()])
        .expect("no entry of this crate's collides with a built-in");
    assert!(
        registry.lookup("log").is_some(),
        "the command a console would reach",
    );
}
