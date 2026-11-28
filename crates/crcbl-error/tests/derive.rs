//! What `#[derive(Error)]` expands to, observed through the impls it writes:
//! the `Display` text, the `source()` chain, the `From` conversions, and
//! `transparent`'s forwarding of both.
//!
//! The message forms are the ones the workspace's error types use — each test
//! names the shape it stands for — so a form that stopped expanding the way
//! `thiserror` did would change a real crate's message.

use std::error::Error as _;
use std::path::{Path, PathBuf};

use crcbl_error::Error;

/// The error a source chain bottoms out in, written by hand so the tests do
/// not depend on the derive to build their fixtures.
#[derive(Debug)]
struct Leaf;

impl std::fmt::Display for Leaf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("leaf")
    }
}

impl std::error::Error for Leaf {}

/// A hand-written error with a source, for `transparent` to forward through.
#[derive(Debug)]
struct Middle(Leaf);

impl std::fmt::Display for Middle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("middle")
    }
}

impl std::error::Error for Middle {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// A source's `Display`, or `None` — the observable a chain test reads.
fn source_text(error: &dyn std::error::Error) -> Option<String> {
    error.source().map(ToString::to_string)
}

#[derive(Debug, Error)]
enum Formats {
    #[error("a plain message")]
    Unit,
    #[error("the platform refused a thread for {name}")]
    Named { name: &'static str },
    #[error("tuple {0} then {1}")]
    Tuple(u32, &'static str),
    #[error("debug {value:?} and tuple debug")]
    Debug { value: Option<u8> },
    #[error("hex {0:#x}, padded {0:#04x}, wide {0:#010x}, bare {0:x}, zero {0:02x}")]
    Hex(u32),
    #[error("{}: {reason}", .path.display())]
    DotField { path: PathBuf, reason: String },
    #[error("{} then {}", .0.1, .1)]
    DotIndex((u8, u8), u8),
    #[error("{} within {}", a.0, limit.max)]
    FieldExpression { a: (u8,), limit: Limits },
    #[error("rate {rate} outside {min}..={max}", min = 8, max = 48)]
    NamedArguments { rate: u32 },
    #[error("{count} shadowed", count = 7)]
    Shadowed { count: u32 },
    #[error(
        "{} {}",
        if *flag { "present" } else { "absent" },
        names.iter().map(|n| n.to_uppercase()).collect::<Vec<_>>().join(",")
    )]
    Expressions { flag: bool, names: Vec<String> },
    #[error("literal {{braces}} and {{{0}}}")]
    Escapes(u8),
    #[error("{path}: corrupt, and {path:?} quoted")]
    PathField { path: PathBuf },
    #[error("borrowed {0}")]
    BorrowedPath(&'static Path),
    #[error("{path} is the argument's", path = "the named argument")]
    PathShadowed { path: PathBuf },
}

#[derive(Debug)]
struct Limits {
    max: u32,
}

#[test]
fn messages_read_as_thiserror_wrote_them() {
    let cases: [(Formats, &str); 16] = [
        (Formats::Unit, "a plain message"),
        (
            Formats::Named { name: "worker-3" },
            "the platform refused a thread for worker-3",
        ),
        (Formats::Tuple(4, "four"), "tuple 4 then four"),
        (
            Formats::Debug { value: Some(2) },
            "debug Some(2) and tuple debug",
        ),
        (
            Formats::Hex(0xab),
            "hex 0xab, padded 0xab, wide 0x000000ab, bare ab, zero ab",
        ),
        (
            Formats::DotField {
                path: PathBuf::from("a/b.vdf"),
                reason: "missing".into(),
            },
            "a/b.vdf: missing",
        ),
        (Formats::DotIndex((5, 6), 7), "6 then 7"),
        (
            Formats::FieldExpression {
                a: (3,),
                limit: Limits { max: 9 },
            },
            "3 within 9",
        ),
        (
            Formats::NamedArguments { rate: 96 },
            "rate 96 outside 8..=48",
        ),
        (Formats::Shadowed { count: 1 }, "7 shadowed"),
        (
            Formats::Expressions {
                flag: true,
                names: vec!["a".into(), "b".into()],
            },
            "present A,B",
        ),
        (Formats::Escapes(1), "literal {braces} and {1}"),
        (
            Formats::PathField {
                path: PathBuf::from("save/slot.ron"),
            },
            "save/slot.ron: corrupt, and \"save/slot.ron\" quoted",
        ),
        (Formats::BorrowedPath(Path::new("x/y")), "borrowed x/y"),
        (
            Formats::PathShadowed {
                path: PathBuf::from("unused"),
            },
            "the named argument is the argument's",
        ),
        (
            Formats::Expressions {
                flag: false,
                names: Vec::new(),
            },
            "absent ",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected, "{error:?}");
    }
}

#[derive(Debug, Error)]
#[error("no adapter matches {pin:?} (of {count})")]
struct NamedStruct {
    pin: &'static str,
    count: usize,
}

#[derive(Debug, Error)]
#[error("pin {0} missed")]
struct TupleStruct(u8);

#[derive(Debug, Error)]
#[error("unit struct")]
struct UnitStruct;

#[test]
fn a_struct_takes_its_message_from_the_struct() {
    let named = NamedStruct {
        pin: "gpu",
        count: 2,
    };
    assert_eq!(named.to_string(), "no adapter matches \"gpu\" (of 2)");
    assert_eq!(TupleStruct(3).to_string(), "pin 3 missed");
    assert_eq!(UnitStruct.to_string(), "unit struct");
    assert!(UnitStruct.source().is_none());
}

#[derive(Debug, Error)]
enum Sources {
    #[error("marked")]
    Marked {
        name: &'static str,
        #[source]
        inner: Leaf,
    },
    #[error("by name")]
    ByName { source: Leaf, detail: String },
    #[error("tuple marked")]
    TupleMarked(u8, #[source] Leaf),
    #[error("none")]
    None { detail: String },
    #[error("unit, none")]
    Unit,
}

#[test]
fn source_returns_the_marked_or_the_source_named_field() {
    let marked = Sources::Marked {
        name: "n",
        inner: Leaf,
    };
    assert_eq!(source_text(&marked).as_deref(), Some("leaf"));
    let by_name = Sources::ByName {
        source: Leaf,
        detail: String::new(),
    };
    assert_eq!(source_text(&by_name).as_deref(), Some("leaf"));
    assert_eq!(
        source_text(&Sources::TupleMarked(0, Leaf)).as_deref(),
        Some("leaf")
    );
    let none = Sources::None {
        detail: String::new(),
    };
    assert_eq!(source_text(&none), None);
    assert_eq!(source_text(&Sources::Unit), None);
}

/// A field named `source` is the source only when no field is marked: the
/// marked one wins, as thiserror has it.
#[derive(Debug, Error)]
#[error("marked wins")]
struct MarkedBeatsName {
    #[source]
    marked: Middle,
    source: Leaf,
}

#[test]
fn a_marked_source_wins_over_a_field_named_source() {
    let error = MarkedBeatsName {
        marked: Middle(Leaf),
        source: Leaf,
    };
    assert_eq!(source_text(&error).as_deref(), Some("middle"));
}

/// A type with no source at all writes no `source`, and the trait's own
/// `None` stands.
#[derive(Debug, Error)]
#[error("{0}")]
struct NoSource(&'static str);

#[test]
fn a_type_with_no_source_returns_none() {
    assert!(NoSource("x").source().is_none());
}

#[derive(Debug, Error)]
enum Froms {
    #[error("leaf in a tuple")]
    Tuple(#[from] Leaf),
    #[error("middle in a struct variant")]
    Named {
        #[from]
        middle: Middle,
    },
    #[error("parse: {0}")]
    Parse(#[from] std::num::ParseIntError),
}

#[derive(Debug, Error)]
#[error("struct from")]
struct FromStruct(#[from] Leaf);

#[test]
fn from_builds_the_variant_and_the_field_is_the_source() {
    let tuple = Froms::from(Leaf);
    assert!(matches!(tuple, Froms::Tuple(Leaf)));
    assert_eq!(source_text(&tuple).as_deref(), Some("leaf"));

    let named: Froms = Middle(Leaf).into();
    assert!(matches!(
        named,
        Froms::Named {
            middle: Middle(Leaf)
        }
    ));
    assert_eq!(source_text(&named).as_deref(), Some("middle"));

    let parse: Froms = "z".parse::<u8>().unwrap_err().into();
    assert_eq!(parse.to_string(), "parse: invalid digit found in string");

    let from_struct: FromStruct = Leaf.into();
    assert_eq!(source_text(&from_struct).as_deref(), Some("leaf"));
}

#[test]
fn question_mark_converts_through_from() {
    fn parse(text: &str) -> Result<u8, Froms> {
        Ok(text.parse::<u8>()?)
    }
    assert!(matches!(parse("x"), Err(Froms::Parse(_))));
    assert_eq!(parse("4").ok(), Some(4));
}

#[derive(Debug, Error)]
enum Transparent {
    #[error(transparent)]
    Wrapped(#[from] Middle),
    #[error(transparent)]
    Named { inner: Leaf },
    #[error("not transparent")]
    Own(#[source] Middle),
}

#[derive(Debug, Error)]
#[error(transparent)]
struct TransparentStruct(Middle);

#[test]
fn transparent_forwards_display_and_source() {
    let wrapped = Transparent::from(Middle(Leaf));
    // The wrapper is invisible: its text is the inner error's, and its source
    // is the inner error's source, not the inner error itself.
    assert_eq!(wrapped.to_string(), "middle");
    assert_eq!(source_text(&wrapped).as_deref(), Some("leaf"));

    let named = Transparent::Named { inner: Leaf };
    assert_eq!(named.to_string(), "leaf");
    assert_eq!(source_text(&named), None);

    let own = Transparent::Own(Middle(Leaf));
    assert_eq!(own.to_string(), "not transparent");
    assert_eq!(source_text(&own).as_deref(), Some("middle"));

    let structure = TransparentStruct(Middle(Leaf));
    assert_eq!(structure.to_string(), "middle");
    assert_eq!(source_text(&structure).as_deref(), Some("leaf"));
}

#[derive(Debug, Error)]
enum Generic<'a, T: std::fmt::Display + std::fmt::Debug, E>
where
    E: std::error::Error + 'static,
{
    #[error("value {0} at {1}")]
    Value(T, &'a str),
    #[error("wrapped")]
    Wrapped(#[source] E),
}

#[test]
fn generic_types_carry_their_generics_and_where_clause() {
    let value: Generic<'_, u8, Leaf> = Generic::Value(3, "here");
    assert_eq!(value.to_string(), "value 3 at here");
    assert_eq!(source_text(&value), None);
    let wrapped: Generic<'_, u8, Leaf> = Generic::Wrapped(Leaf);
    assert_eq!(source_text(&wrapped).as_deref(), Some("leaf"));
}

/// An empty enum has no value, so its impls only have to compile.
#[derive(Debug, Error)]
enum Never {}

#[test]
fn an_empty_enum_derives() {
    fn is_error<E: std::error::Error>() {}
    is_error::<Never>();
}

/// The expansion names only absolute paths, so names in scope where the type
/// is declared — a local `Result`, `Option`, `fmt`, `Error` or `core` — change
/// nothing.
mod shadowed {
    #![allow(dead_code)]

    type Result = ();
    type Option = ();
    struct Some;
    mod fmt {}
    mod core {}
    trait Display {}

    #[derive(Debug, crcbl_error::Error)]
    pub(crate) enum Shadowed {
        #[error("shadowed {0}")]
        One(u8),
        #[error(transparent)]
        Two(#[from] super::Leaf),
    }
}

#[test]
fn local_names_do_not_capture_the_expansion() {
    assert_eq!(shadowed::Shadowed::One(1).to_string(), "shadowed 1");
    let two = shadowed::Shadowed::from(Leaf);
    assert_eq!(two.to_string(), "leaf");
}
