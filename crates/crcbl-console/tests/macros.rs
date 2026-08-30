//! What the declarative macros produce.
//!
//! An integration test rather than a unit one, and deliberately: the guard in
//! `tests/guard.rs` reads this crate's `src/` for declarations and holds each to
//! the built-in table, so a declaration written to exercise a macro must live
//! somewhere that scan does not look.

use std::any::Any;

use crcbl_console::{Binding, Fault, Flags, Kind, Value, concommand, convar, table};

convar! {
    /// Draw the ambient-occlusion channel as grey
    /// instead of the shaded frame.
    pub static r_ao_view: bool = false;
}

convar! {
    /// How many samples the anisotropic filter takes.
    #[flags(ARCHIVE)]
    pub static anisotropic_filtering: i64 in 1..=16 = 1;
}

convar! {
    /// The fraction of the frame the renderer draws at.
    #[flags(ARCHIVE | READ_ONLY)]
    pub static render_scale: f32 in 0.25..=2.0 = 1.0;
}

convar! {
    /// Which edge-antialiasing pass runs.
    #[flags(ARCHIVE)]
    pub static antialiasing: &'static str one_of ["none", "fxaa", "smaa"] = "fxaa";
}

// The plan writes code-declared variables in SCREAMING_CASE; Source writes them
// the way they are typed. Both compile, and the registry treats them as one
// name — see `the_declared_case_is_kept_and_matched_without_regard_to_it` in
// tests/registry.rs.
convar! {
    /// A variable declared in the plan's spelling.
    pub static R_SHOUTED: bool = true;
}

concommand! {
    /// Print the arguments back, loudly.
    pub fn shout(cx, args) {
        cx.print(args.join(" ").to_uppercase());
        Ok(())
    }
}

concommand! {
    /// Take neither of its parameters.
    pub fn nop(_cx, _args) {
        Ok(())
    }
}

fn nothing(_host: &dyn Any) -> Value {
    Value::Text(String::new())
}

fn refuse(_host: &mut dyn Any, _value: &Value) -> Result<(), Fault> {
    Err(Fault::new("nothing writes this"))
}

static A_BINDING: Binding = Binding::new(
    "player_name",
    "the name a text variable can only have as a binding",
    Kind::Text,
    Flags::ARCHIVE,
    nothing,
    refuse,
);

#[test]
fn a_declared_variables_help_is_its_doc_comment() {
    assert_eq!(
        r_ao_view.help(),
        "Draw the ambient-occlusion channel as grey instead of the shaded frame."
    );
    assert_eq!(
        anisotropic_filtering.help(),
        "How many samples the anisotropic filter takes."
    );
    assert_eq!(shout.help(), "Print the arguments back, loudly.");
}

#[test]
fn a_declared_entrys_name_is_its_ident() {
    assert_eq!(r_ao_view.name(), "r_ao_view");
    assert_eq!(anisotropic_filtering.name(), "anisotropic_filtering");
    assert_eq!(R_SHOUTED.name(), "R_SHOUTED");
    assert_eq!(shout.name(), "shout");
    assert_eq!(nop.name(), "nop");
}

#[test]
fn the_type_in_the_declaration_is_the_kind() {
    assert_eq!(r_ao_view.kind(), Kind::Bool);
    assert_eq!(anisotropic_filtering.kind(), Kind::Int { min: 1, max: 16 });
    assert_eq!(
        render_scale.kind(),
        Kind::Float {
            min: 0.25,
            max: 2.0
        }
    );
    assert_eq!(antialiasing.kind(), Kind::Enum(&["none", "fxaa", "smaa"]));
}

#[test]
fn the_declared_default_is_the_starting_value() {
    assert!(!r_ao_view.get_bool());
    assert_eq!(anisotropic_filtering.get_i64(), 1);
    assert_eq!(render_scale.get_f32(), 1.0);
    // An enum default is written as the name, and resolves to its index.
    assert_eq!(antialiasing.get_enum(), "fxaa");
    assert_eq!(*antialiasing.default(), Value::Enum("fxaa"));
}

#[test]
fn the_flags_attribute_is_optional_and_takes_a_set() {
    assert_eq!(r_ao_view.flags(), Flags::NONE);
    assert_eq!(anisotropic_filtering.flags(), Flags::ARCHIVE);
    assert_eq!(render_scale.flags(), Flags::ARCHIVE | Flags::READ_ONLY);
    assert!(render_scale.flags().contains(Flags::READ_ONLY));
}

#[test]
fn a_read_only_declaration_refuses_a_set() {
    assert_eq!(
        render_scale
            .set(&Value::Float(0.5))
            .expect_err("read only")
            .message(),
        "`render_scale` is read-only"
    );
    assert_eq!(render_scale.get_f32(), 1.0);
}

#[test]
fn a_table_sorts_its_entries_into_the_three_lists_whatever_order_they_are_in() {
    let built = table![
        r_ao_view,
        cmd shout,
        bind A_BINDING,
        anisotropic_filtering,
        cmd nop,
    ];
    assert_eq!(built.vars().len(), 2);
    assert_eq!(built.bindings().len(), 1);
    assert_eq!(built.commands().len(), 2);
    assert_eq!(built.len(), 5);
    assert!(!built.is_empty());
    assert_eq!(built.vars()[0].name(), "r_ao_view");
    assert_eq!(built.bindings()[0].name(), "player_name");
    assert_eq!(built.commands()[1].name(), "nop");
}

#[test]
fn a_table_with_nothing_in_it_is_empty() {
    let built = table![];
    assert!(built.is_empty());
    assert_eq!(built.len(), 0);
    assert!(crcbl_console::Table::EMPTY.is_empty());
}

#[test]
fn a_table_is_also_a_constant() {
    static TABLE: crcbl_console::Table = table![r_ao_view, cmd shout];
    assert_eq!(TABLE.len(), 2);
}
