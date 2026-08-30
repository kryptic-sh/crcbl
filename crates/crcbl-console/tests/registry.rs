//! Gathering a registry, running a line through it, and completing one.
//!
//! An integration test for `tests/macros.rs`'s reason: the declarations below
//! would otherwise be entries this crate's own guard demands of its built-in
//! table.

use std::any::Any;

use crcbl_console::{
    Binding, Context, Entry, Fault, Flags, Kind, Registry, Table, Value, concommand, convar, table,
};

convar! {
    /// Draw the ambient-occlusion channel as grey instead of the shaded frame.
    pub static r_ao_view: bool = false;
}

convar! {
    /// Which edge-antialiasing pass runs.
    #[flags(ARCHIVE)]
    pub static antialiasing: &'static str one_of ["none", "fxaa", "smaa"] = "none";
}

convar! {
    /// How many samples the anisotropic filter takes.
    #[flags(ARCHIVE)]
    pub static anisotropic_filtering: i64 in 1..=16 = 1;
}

convar! {
    /// The vertical field of view. Nothing reads this yet.
    #[flags(READ_ONLY)]
    pub static fov: f32 in 1.0..=179.0 = 90.0;
}

convar! {
    /// A variable declared the way plan 52 writes one.
    pub static R_SHOUTED: bool = true;
}

convar! {
    /// Which channel the renderer draws, with a value that holds a space.
    pub static t_debug_view: &'static str one_of ["shaded", "ambient occlusion", "lod tint"] =
        "shaded";
}

// Three variables no other test reads. The tests in a binary run in parallel
// and a `ConVar` is a process-wide static, so a test that writes one and a test
// that asserts its value are a race unless they are about different variables.
convar! {
    /// A set of names one test writes.
    pub static t_written_enum: &'static str one_of ["none", "fxaa", "smaa"] = "none";
}

convar! {
    /// A whole number one test writes.
    pub static t_written_int: i64 in 1..=16 = 1;
}

convar! {
    /// A whole number one test tries to write out of range.
    pub static t_refused_int: i64 in 1..=16 = 1;
}

// `toggle` and `reset`'s own variables, for the reason above and one more: a
// bare `reset` walks the whole registry, so it is run over a table of its own
// rather than over `a_table`, where it would put back a value another test in
// this binary is in the middle of asserting.
convar! {
    /// A switch one test flips.
    pub static t_toggled: bool = false;
}

convar! {
    /// A whole number one test resets by name.
    pub static t_reset_int: i64 in 1..=16 = 4;
}

convar! {
    /// A whole number one test tries to toggle. Nothing ever writes it.
    pub static t_not_a_bool: i64 in 0..=9 = 3;
}

convar! {
    /// A switch the bare reset puts back.
    pub static t_bare_switch: bool = false;
}

convar! {
    /// A saved variable the bare reset must leave exactly where it is.
    #[flags(ARCHIVE)]
    pub static t_bare_saved: i64 in 1..=16 = 1;
}

concommand! {
    /// Print the arguments back, loudly.
    pub fn shout(cx, args) {
        cx.print(args.join(" ").to_uppercase());
        Ok(())
    }
}

concommand! {
    /// Refuse, and leave everything alone.
    pub fn refuse(_cx, _args) {
        Err(Fault::new("refused on purpose"))
    }
}

/// The host state a binding writes, standing in for slice 2's settings stack.
#[derive(Debug, Default)]
struct Host {
    volume: f32,
}

fn volume_of(host: &dyn Any) -> Value {
    Value::Float(host.downcast_ref::<Host>().expect("a Host").volume)
}

fn set_volume(host: &mut dyn Any, value: &Value) -> Result<(), Fault> {
    let Value::Float(v) = value else {
        return Err(Fault::new("`volume` is a float"));
    };
    host.downcast_mut::<Host>().expect("a Host").volume = *v;
    Ok(())
}

static VOLUME: Binding = Binding::new(
    "volume",
    "the master gain applied to every mixer bus",
    Kind::Float { min: 0.0, max: 1.0 },
    Flags::ARCHIVE,
    volume_of,
    set_volume,
);

fn a_table() -> Table {
    table![
        r_ao_view,
        antialiasing,
        anisotropic_filtering,
        fov,
        R_SHOUTED,
        t_debug_view,
        t_written_enum,
        t_written_int,
        t_refused_int,
        t_toggled,
        t_reset_int,
        t_not_a_bool,
        bind VOLUME,
        cmd shout,
        cmd refuse,
    ]
}

fn gathered() -> Registry {
    Registry::gather(&[a_table()]).expect("no two entries claim one name")
}

/// A registry holding only the variables the bare `reset` is allowed to walk.
fn reset_registry() -> Registry {
    Registry::gather(&[table![t_bare_switch, t_bare_saved]]).expect("no two entries claim one name")
}

/// Run `line` and return what it printed.
fn run(registry: &Registry, line: &str) -> Vec<String> {
    let mut host = Host::default();
    let mut cx = Context::new(registry, &mut host);
    registry.execute(&mut cx, line).expect("the line runs");
    cx.into_lines()
}

// -- gather ------------------------------------------------------------------

#[test]
fn a_gathered_registry_is_sorted_by_name_without_regard_to_case() {
    let registry = gathered();
    let names: Vec<&str> = registry.entries().iter().map(|e| e.name()).collect();
    let mut sorted = names.clone();
    sorted.sort_by_key(|name| name.to_ascii_lowercase());
    assert_eq!(names, sorted);
    // Both lists are sorted on their own too, since both are handed out.
    let vars: Vec<&str> = registry.vars().iter().map(|var| var.name()).collect();
    let mut sorted_vars = vars.clone();
    sorted_vars.sort_by_key(|name| name.to_ascii_lowercase());
    assert_eq!(vars, sorted_vars);
    let commands: Vec<&str> = registry.commands().iter().map(|c| c.name()).collect();
    let mut sorted_commands = commands.clone();
    sorted_commands.sort_by_key(|name| name.to_ascii_lowercase());
    assert_eq!(commands, sorted_commands);
}

#[test]
fn the_built_in_commands_are_in_every_registry() {
    let registry = Registry::gather(&[]).expect("the built-ins alone do not collide");
    for name in ["help", "find", "echo", "clear"] {
        assert!(registry.lookup(name).is_some(), "{name}");
    }
    assert!(registry.vars().is_empty());
}

#[test]
fn two_entries_claiming_one_name_are_refused_and_both_are_named() {
    let duplicate = Registry::gather(&[a_table(), a_table()]).expect_err("the same table twice");
    let printed = duplicate.to_string();
    assert!(
        printed.contains("two console entries claim the name"),
        "{printed}"
    );
    assert_eq!(duplicate.first.name(), duplicate.second.name());
    assert!(printed.contains(duplicate.first.help()), "{printed}");
}

#[test]
fn a_command_and_a_variable_cannot_share_a_name_either() {
    // `echo` is a built-in command; a variable of that name collides with it.
    let duplicate =
        Registry::gather(&[table![cmd shout, cmd shout]]).expect_err("one name declared twice");
    assert_eq!(duplicate.name, "shout");
}

#[test]
fn passing_the_built_in_table_as_well_is_the_duplicate_the_docs_warn_of() {
    let duplicate = Registry::gather(&[crcbl_console::builtin_table()])
        .expect_err("the built-ins are added by gather");
    assert!(["help", "find", "echo", "clear"].contains(&duplicate.name.as_str()));
}

#[test]
fn the_declared_case_is_kept_and_matched_without_regard_to_it() {
    let registry = gathered();
    let entry = registry.lookup("r_shouted").expect("found in any case");
    assert_eq!(entry.name(), "R_SHOUTED");
    assert!(registry.lookup("ANTIALIASING").is_some());
    assert!(registry.lookup("no_such_thing").is_none());
}

// -- find --------------------------------------------------------------------

#[test]
fn find_is_a_substring_over_names_and_over_help() {
    let registry = gathered();
    let by_name: Vec<&str> = registry.find("alias").iter().map(|e| e.name()).collect();
    assert_eq!(by_name, ["antialiasing"]);

    // "mixer" appears only in the binding's help.
    let by_help: Vec<&str> = registry.find("mixer").iter().map(|e| e.name()).collect();
    assert_eq!(by_help, ["volume"]);

    // And it ignores case on both sides.
    assert_eq!(registry.find("ALIAS").len(), 1);
    assert!(registry.find("no such text anywhere").is_empty());
}

// -- execute -----------------------------------------------------------------

#[test]
fn a_bare_variable_prints_its_value_its_default_its_flags_and_its_help() {
    let registry = gathered();
    assert_eq!(
        run(&registry, "antialiasing"),
        ["antialiasing = none (default: none) [ARCHIVE] — Which edge-antialiasing pass runs."]
    );
    assert_eq!(
        run(&registry, "r_ao_view"),
        [concat!(
            "r_ao_view = false (default: false) — Draw the ambient-occlusion ",
            "channel as grey instead of the shaded frame."
        )]
    );
}

#[test]
fn a_bound_variable_prints_the_hosts_value_and_has_no_default_to_show() {
    let registry = gathered();
    assert_eq!(
        run(&registry, "volume"),
        ["volume = 0 [ARCHIVE] — the master gain applied to every mixer bus"]
    );
}

#[test]
fn setting_a_variable_prints_the_new_value_and_the_owning_code_reads_it() {
    let registry = gathered();
    assert_eq!(
        run(&registry, "t_written_enum smaa"),
        ["t_written_enum = smaa"]
    );
    assert_eq!(t_written_enum.get_enum(), "smaa");
    // The `=` is optional and means the same thing.
    assert_eq!(
        run(&registry, "t_written_enum = fxaa"),
        ["t_written_enum = fxaa"]
    );
    assert_eq!(t_written_enum.get_enum(), "fxaa");
    assert_eq!(run(&registry, "t_written_int 8"), ["t_written_int = 8"]);
    assert_eq!(t_written_int.get_i64(), 8);
}

#[test]
fn setting_a_bound_variable_writes_the_host() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    registry
        .execute(&mut cx, "volume 0.5")
        .expect("inside the range");
    assert_eq!(cx.lines(), ["volume = 0.5"]);
    drop(cx);
    assert_eq!(host.volume, 0.5);
}

#[test]
fn a_value_the_kind_refuses_leaves_the_variable_alone() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    let fault = registry
        .execute(&mut cx, "t_refused_int 64")
        .expect_err("outside the range");
    assert_eq!(fault.message(), "64 is outside 1..=16");
    assert_eq!(t_refused_int.get_i64(), 1);
    assert!(cx.lines().is_empty());
}

#[test]
fn a_read_only_variable_prints_and_refuses_a_set() {
    let registry = gathered();
    assert_eq!(
        run(&registry, "fov"),
        [
            "fov = 90 (default: 90) [READ_ONLY] — The vertical field of view. Nothing reads this yet."
        ]
    );
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    assert_eq!(
        registry
            .execute(&mut cx, "fov 100")
            .expect_err("read only")
            .message(),
        "`fov` is read-only"
    );
    assert_eq!(fov.get_f32(), 90.0);
}

#[test]
fn an_unknown_name_is_refused_with_a_message_that_points_at_find() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    let fault = registry
        .execute(&mut cx, "r_no_such_variable")
        .expect_err("nothing knows that name");
    assert_eq!(
        fault.message(),
        "unknown command or variable `r_no_such_variable` — try `find r_no_such_variable`"
    );
}

#[test]
fn several_statements_run_in_order_and_the_first_fault_stops_the_line() {
    let registry = gathered();
    assert_eq!(
        run(&registry, "echo one; echo two; echo three"),
        ["one", "two", "three"]
    );

    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    let fault = registry
        .execute(&mut cx, "echo before; refuse; echo after")
        .expect_err("the middle statement refuses");
    assert_eq!(fault.message(), "refused on purpose");
    assert_eq!(cx.lines(), ["before"]);
}

#[test]
fn a_command_runs_with_the_arguments_it_was_given() {
    let registry = gathered();
    assert_eq!(run(&registry, "shout hello there"), ["HELLO THERE"]);
    assert_eq!(
        run(&registry, "echo \"two words\" three"),
        ["two words three"]
    );
}

#[test]
fn clear_is_a_request_the_ui_honours_and_prints_nothing() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    assert!(!cx.clear_requested());
    registry.execute(&mut cx, "clear").expect("clear runs");
    assert!(cx.clear_requested());
    assert!(cx.lines().is_empty());
}

#[test]
fn help_lists_every_entry_and_nothing_less() {
    let registry = gathered();
    let printed = run(&registry, "help");
    assert_eq!(
        printed.len(),
        registry.vars().len() + registry.commands().len(),
        "help printed {} lines for {} variables and {} commands",
        printed.len(),
        registry.vars().len(),
        registry.commands().len()
    );
    // And the lines are the shape a bare variable prints, so `help` and a
    // variable on its own cannot disagree.
    assert!(
        printed
            .iter()
            .any(|line| line.starts_with("antialiasing = ")),
        "{printed:?}"
    );
    assert!(
        printed
            .iter()
            .any(|line| line == "echo — Print the arguments back as one line."),
        "{printed:?}"
    );
}

#[test]
fn help_with_a_prefix_lists_the_matches_only() {
    let registry = gathered();
    let printed = run(&registry, "help r_");
    assert_eq!(printed.len(), 2, "{printed:?}");
    assert!(
        printed
            .iter()
            .all(|line| line.to_ascii_lowercase().starts_with("r_"))
    );
}

#[test]
fn find_with_no_text_is_refused_rather_than_listing_everything() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    assert_eq!(
        registry
            .execute(&mut cx, "find")
            .expect_err("nothing to look for")
            .message(),
        "find needs some text to look for"
    );
}

#[test]
fn the_find_command_prints_what_the_registry_finds() {
    let registry = gathered();
    let printed = run(&registry, "find mixer");
    assert_eq!(printed.len(), 1, "{printed:?}");
    assert!(printed[0].starts_with("volume = "), "{printed:?}");
}

#[test]
fn an_empty_line_runs_nothing() {
    let registry = gathered();
    assert!(run(&registry, "").is_empty());
    assert!(run(&registry, "   ;  ; ").is_empty());
}

// -- completion --------------------------------------------------------------

#[test]
fn completing_a_name_fills_in_the_prefix_every_candidate_shares() {
    let registry = gathered();
    let completion = registry.complete("anti");
    assert_eq!(completion.candidates, ["antialiasing"]);
    assert_eq!(completion.common, "antialiasing");

    let completion = registry.complete("an");
    assert_eq!(
        completion.candidates,
        ["anisotropic_filtering", "antialiasing"]
    );
    assert_eq!(completion.common, "an");
}

#[test]
fn the_candidates_are_sorted_and_case_is_ignored_both_ways() {
    let registry = gathered();
    let completion = registry.complete("R_");
    assert_eq!(completion.candidates, ["r_ao_view", "R_SHOUTED"]);
    // The fill is the declared spelling, not the typed one.
    assert_eq!(completion.common, "r_");

    let completion = registry.complete("r_a");
    assert_eq!(completion.common, "r_ao_view");
}

#[test]
fn completing_nothing_offers_every_entry() {
    let registry = gathered();
    let completion = registry.complete("");
    assert_eq!(completion.candidates.len(), registry.entries().len());
    assert_eq!(completion.common, "");
}

#[test]
fn a_prefix_nothing_starts_with_offers_nothing() {
    let registry = gathered();
    let completion = registry.complete("zzz");
    assert!(completion.candidates.is_empty());
    assert_eq!(completion.common, "");
}

#[test]
fn an_enum_variable_completes_its_values() {
    let registry = gathered();
    let completion = registry.complete("antialiasing ");
    assert_eq!(completion.candidates, ["fxaa", "none", "smaa"]);
    assert_eq!(completion.common, "");

    let completion = registry.complete("antialiasing s");
    assert_eq!(completion.candidates, ["smaa"]);
    assert_eq!(completion.common, "smaa");

    let completion = registry.complete("ANTIALIASING F");
    assert_eq!(completion.candidates, ["fxaa"]);
}

#[test]
fn an_enum_value_that_holds_a_space_is_one_value_to_set_and_to_complete() {
    let registry = gathered();
    let completion = registry.complete("t_debug_view ");
    assert_eq!(
        completion.candidates,
        ["ambient occlusion", "lod tint", "shaded"]
    );
    let completion = registry.complete("t_debug_view ambient o");
    assert_eq!(completion.candidates, ["ambient occlusion"]);
    assert_eq!(completion.common, "ambient occlusion");

    assert_eq!(
        run(&registry, "t_debug_view ambient occlusion"),
        ["t_debug_view = ambient occlusion"]
    );
    assert_eq!(t_debug_view.get_enum(), "ambient occlusion");
}

#[test]
fn a_variable_with_no_set_of_values_completes_nothing_after_its_name() {
    let registry = gathered();
    assert_eq!(
        registry.complete("r_ao_view "),
        crcbl_console::Completion::default()
    );
    assert_eq!(
        registry.complete("shout hel"),
        crcbl_console::Completion::default()
    );
    assert_eq!(
        registry.complete("antialiasing smaa "),
        crcbl_console::Completion::default()
    );
}

// -- entries -----------------------------------------------------------------

#[test]
fn a_context_debug_prints_what_has_been_printed_through_it() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    registry.execute(&mut cx, "echo hello").expect("echo runs");
    let printed = format!("{cx:?}");
    assert!(printed.contains("hello"), "{printed}");
    assert!(printed.contains("clear"), "{printed}");
}

#[test]
fn a_duplicate_is_an_error_in_its_own_right() {
    let duplicate = Registry::gather(&[a_table(), a_table()]).expect_err("the same table twice");
    let as_error: &dyn std::error::Error = &duplicate;
    assert_eq!(as_error.to_string(), duplicate.to_string());
}

#[test]
fn an_entry_answers_for_the_thing_it_holds() {
    let registry = gathered();
    let Some(Entry::Var(var)) = registry.lookup("volume") else {
        panic!("`volume` is a variable");
    };
    assert_eq!(var.name(), "volume");
    assert!(var.default().is_none());

    let Some(Entry::Command(command)) = registry.lookup("echo") else {
        panic!("`echo` is a command");
    };
    assert_eq!(command.help(), "Print the arguments back as one line.");
}

// -- toggle and reset --------------------------------------------------------

/// **`toggle` flips the cell the owning code reads**, which is the whole point
/// of a `ConVar` being its own storage: the value is read back off the static
/// and not off the line the command printed.
#[test]
fn toggle_flips_a_bool_and_the_owning_code_reads_the_new_value() {
    let registry = gathered();
    assert!(!t_toggled.get_bool(), "declared false");
    assert_eq!(run(&registry, "toggle t_toggled"), ["t_toggled = true"]);
    assert!(t_toggled.get_bool(), "the flip did not reach the cell");
    assert_eq!(run(&registry, "toggle t_toggled"), ["t_toggled = false"]);
    assert!(!t_toggled.get_bool(), "and back again");
}

/// **A variable that is not a bool has nothing to flip**, and the refusal says
/// what it is instead.
#[test]
fn toggle_refuses_a_variable_that_is_not_a_bool() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    let fault = registry
        .execute(&mut cx, "toggle t_not_a_bool")
        .expect_err("an int is not a bool");
    assert_eq!(
        fault.message(),
        "`t_not_a_bool` is an int, and toggle is for a bool"
    );
    assert_eq!(t_not_a_bool.get_i64(), 3, "and it left it alone");
    assert!(cx.lines().is_empty());
}

/// **A command is refused by name**, rather than reported as an unknown
/// variable — it exists, and sending someone to `find` would send them looking
/// for a typo that is not there.
#[test]
fn toggle_names_a_command_rather_than_calling_it_unknown() {
    let registry = gathered();
    let mut host = Host::default();
    let mut cx = Context::new(&registry, &mut host);
    assert_eq!(
        registry
            .execute(&mut cx, "toggle echo")
            .expect_err("`echo` is a command")
            .message(),
        "`echo` is a command, not a variable"
    );
    assert_eq!(
        registry
            .execute(&mut cx, "toggle r_no_such_variable")
            .expect_err("nothing knows that name")
            .message(),
        "unknown variable `r_no_such_variable` — try `find r_no_such_variable`"
    );
}

/// **`reset <var>` puts the declared default back into the cell.**
#[test]
fn reset_puts_one_variable_back_to_its_declared_default() {
    let registry = gathered();
    assert_eq!(run(&registry, "t_reset_int 9"), ["t_reset_int = 9"]);
    assert_eq!(t_reset_int.get_i64(), 9);
    assert_eq!(run(&registry, "reset t_reset_int"), ["t_reset_int = 4"]);
    assert_eq!(
        t_reset_int.get_i64(),
        4,
        "the default did not reach the cell"
    );
}

/// **A settings-backed variable has no default here to go back to**, and says
/// so rather than writing whatever the console last saw.
#[test]
fn reset_refuses_a_variable_whose_storage_is_the_settings_stack() {
    let registry = gathered();
    let mut host = Host { volume: 0.25 };
    let mut cx = Context::new(&registry, &mut host);
    assert_eq!(
        registry
            .execute(&mut cx, "reset volume")
            .expect_err("a binding declares no default")
            .message(),
        "`volume` is stored through the settings stack, which keeps its own defaults"
    );
    drop(cx);
    assert_eq!(host.volume, 0.25, "and it wrote nothing");
}

/// **A bare `reset` moves the debug variables and leaves the saved ones**, which
/// is plan decision 7's "every non-`ARCHIVE` variable": a console session that
/// emptied the player's settings file would be a preference gone for good.
#[test]
fn a_bare_reset_moves_the_unsaved_variables_and_leaves_the_saved_ones() {
    let registry = reset_registry();
    assert_eq!(
        run(&registry, "t_bare_switch true"),
        ["t_bare_switch = true"]
    );
    assert_eq!(run(&registry, "t_bare_saved 9"), ["t_bare_saved = 9"]);

    assert_eq!(
        run(&registry, "reset"),
        ["t_bare_switch = false"],
        "the bare reset printed something other than the one variable it moved"
    );
    assert!(!t_bare_switch.get_bool(), "the switch did not go back");
    assert_eq!(
        t_bare_saved.get_i64(),
        9,
        "a saved variable was reset with the debug ones"
    );

    // And with nothing left to move it says so, rather than printing nothing
    // at all and looking like a command that did not run.
    assert_eq!(
        run(&registry, "reset"),
        ["every variable is already at its default"]
    );
}
