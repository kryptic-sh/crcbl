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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Json {
    /// `true` / `false`.
    Bool(bool),
    /// A JSON number, always an integer here.
    Number(i64),
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
    fn arrays_nest() {
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

    /// The whole reason this module is allowed to exist rather than being a
    /// `format!` call.
    #[test]
    fn strings_are_escaped() {
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
