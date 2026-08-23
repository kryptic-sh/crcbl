//! A JSON writer, in eighty lines, because `--json` is a contract and
//! `serde_json` is a dependency.
//!
//! `docs/plan/11-cli-headless.md` requires "`--json` on every subcommand" and
//! "stable JSON schemas". What it does not require is a general-purpose
//! serialization framework: this CLI emits objects whose shape is written out
//! by hand a few lines away from the code that decides it, and never *parses*
//! JSON at all. Serializing is the easy direction — escaping is the only part
//! that is subtle, and it is tested here.
//!
//! When the CLI grows the `scene` batch mode from topic 11 it will have to
//! *read* JSON, and that is the moment to reconsider. Writing does not justify
//! it; reading might.

use std::fmt::{self, Display, Formatter, Write as _};

/// A JSON value, built by hand at the call site.
///
/// Ordered rather than a map, because a stable key order makes the output
/// diffable and `--json | head` readable, and because an object with four keys
/// does not need a hash table.
///
/// `PartialEq` but not `Eq`: [`Float`](Self::Float) holds one.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `true` / `false`.
    Bool(bool),
    /// A JSON number, always an integer here.
    Number(i64),
    /// A JSON number with a fraction — a geometric error, a ratio.
    ///
    /// **A non-finite value is written `null`.** JSON has no infinity and no
    /// NaN, so the alternatives are a document no parser accepts or a number
    /// that is not the value; `null` is the one answer a consumer can read and
    /// tell apart from a real measurement. Nothing this CLI reports is
    /// *expected* to be non-finite — see [`write_float`].
    Float(f32),
    /// A JSON number with a fraction that came in as an `f64`.
    ///
    /// Separate from [`Float`](Self::Float) rather than replacing it, because
    /// the two round-trip differently: `Debug` prints the shortest decimal that
    /// reads back as the *same bits*, and `0.03f32` widened to an `f64` is
    /// `0.029999999329447746`. A measurement this CLI computes in single
    /// precision has to stay single precision on the way out, and a value read
    /// from a TOML file — where a float is an `f64` by definition — has to keep
    /// every digit the file spelled. `null` for a non-finite value, on
    /// [`Float`](Self::Float)'s terms.
    Double(f64),
    /// A string, escaped on the way out.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object, in insertion order.
    Object(Vec<(&'static str, Json)>),
}

impl Json {
    /// A string value, from anything that can become one.
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    /// An array of strings, the shape most of this CLI's lists have.
    pub fn strings(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Array(values.into_iter().map(Self::string).collect())
    }
}

impl Display for Json {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Number(value) => write!(f, "{value}"),
            Self::Float(value) => write_float(f, value.is_finite(), value),
            Self::Double(value) => write_float(f, value.is_finite(), value),
            Self::String(value) => write_escaped(f, value),
            Self::Array(items) => {
                f.write_char('[')?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_char(',')?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_char(']')
            }
            Self::Object(fields) => {
                f.write_char('{')?;
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        f.write_char(',')?;
                    }
                    write_escaped(f, key)?;
                    f.write_char(':')?;
                    write!(f, "{value}")?;
                }
                f.write_char('}')
            }
        }
    }
}

/// Writes a JSON number for a float, or `null` when there is no such number.
///
/// `{:?}` and not `{}`: `Debug` for a float is the shortest decimal that reads
/// back as the same bits, which is exactly what a machine-readable schema wants
/// — `Display` would print `0.03` for a value that is not 0.03. Both an integral
/// value (`2.0`) and an exponent (`1e-8`) are spellings RFC 8259 accepts.
///
/// `finite` is passed in rather than tested here: `f32` and `f64` share this
/// rule and share nothing in the type system that would let one function ask
/// them both, and the alternative — a copy of the rule per width — is how the
/// two would come to disagree about what JSON can spell.
fn write_float(f: &mut Formatter<'_>, finite: bool, value: &impl fmt::Debug) -> fmt::Result {
    if finite {
        write!(f, "{value:?}")
    } else {
        f.write_str("null")
    }
}

/// Writes a JSON string literal, escaping everything RFC 8259 requires.
///
/// The control-character case is the one that matters in practice: a Windows
/// path or a compiler diagnostic can carry anything, and an unescaped `\n` in
/// the middle of a string is invalid JSON that most parsers reject at the
/// *end* of the document, where the error is useless.
fn write_escaped(f: &mut Formatter<'_>, value: &str) -> fmt::Result {
    f.write_char('"')?;
    for character in value.chars() {
        match character {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            '\u{8}' => f.write_str("\\b")?,
            '\u{c}' => f.write_str("\\f")?,
            control if control < ' ' => write!(f, "\\u{:04x}", control as u32)?,
            other => f.write_char(other)?,
        }
    }
    f.write_char('"')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_keep_their_key_order() {
        let value = Json::Object(vec![
            ("ok", Json::Bool(true)),
            ("command", Json::string("new")),
            ("count", Json::Number(3)),
        ]);
        assert_eq!(
            value.to_string(),
            r#"{"ok":true,"command":"new","count":3}"#
        );
    }

    #[test]
    fn an_array_nested_in_an_object_renders_inline_and_an_empty_one_is_brackets() {
        let value = Json::Object(vec![(
            "files",
            Json::strings(["Cargo.toml", "src/main.rs"]),
        )]);
        assert_eq!(
            value.to_string(),
            r#"{"files":["Cargo.toml","src/main.rs"]}"#
        );
        assert_eq!(Json::Array(vec![]).to_string(), "[]");
    }

    /// A float is written as the shortest decimal that reads back as itself,
    /// and the three values JSON cannot spell become `null` rather than a
    /// document no parser accepts.
    #[test]
    fn floats_round_trip_and_the_unspellable_ones_are_null() {
        for (value, expected) in [
            (0.0f32, "0.0"),
            (2.0, "2.0"),
            (0.03, "0.03"),
            (-1.5, "-1.5"),
            (1e-8, "1e-8"),
        ] {
            let rendered = Json::Float(value).to_string();
            assert_eq!(rendered, expected);
            assert_eq!(
                rendered.parse::<f32>().expect("a JSON number").to_bits(),
                value.to_bits(),
                "{rendered} is not {value} read back"
            );
        }
        for value in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert_eq!(Json::Float(value).to_string(), "null", "{value}");
        }
    }

    /// A `f64` keeps the digits widening an `f32` would invent, and vice versa.
    ///
    /// The reason the two variants exist. `0.03f32` is not 0.03 — widened to a
    /// `f64` it is 0.029999999329447746, and a shortest-round-trip rendering of
    /// the wide value prints all of it. A TOML float, meanwhile, *is* an `f64`,
    /// so a settings value with more precision than a `f32` holds has to come
    /// out of `crcbl settings get` unchanged. One variant could not do both.
    #[test]
    fn the_two_float_widths_each_round_trip_at_their_own_precision() {
        assert_eq!(Json::Float(0.03).to_string(), "0.03");
        assert_eq!(
            Json::Double(f64::from(0.03f32)).to_string(),
            "0.029999999329447746"
        );

        let precise = 0.123_456_789_012_345_67f64;
        let rendered = Json::Double(precise).to_string();
        assert_eq!(
            rendered.parse::<f64>().expect("a JSON number").to_bits(),
            precise.to_bits(),
            "{rendered} is not {precise} read back"
        );
        assert_ne!(
            Json::Float(precise as f32).to_string(),
            rendered,
            "an f32 cannot hold this value, so the two renderings must differ"
        );

        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(Json::Double(value).to_string(), "null", "{value}");
        }
    }

    /// The whole reason this module is allowed to exist rather than being a
    /// `format!` call.
    #[test]
    fn strings_escape_the_control_characters_json_requires_and_leave_utf8_alone() {
        let nasty = "quote\" back\\slash\nnewline\ttab\u{1}bell\u{8}back";
        let rendered = Json::string(nasty).to_string();
        assert_eq!(
            rendered,
            r#""quote\" back\\slash\nnewline\ttab\u0001bell\bback""#
        );
        // Non-ASCII stays as-is: JSON is UTF-8 and `\u` escaping it would only
        // make the output harder to read.
        assert_eq!(Json::string("é — ✓").to_string(), "\"é — ✓\"");
    }

    #[test]
    fn control_characters_never_leak_through_raw() {
        for code in 0u32..0x20 {
            let character = char::from_u32(code).expect("valid scalar");
            let rendered = Json::string(character.to_string()).to_string();
            assert!(
                !rendered.contains(character),
                "U+{code:04X} was emitted raw: {rendered:?}"
            );
        }
    }
}
