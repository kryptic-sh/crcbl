//! What a console value is, what shapes one may take, and how typed text
//! becomes one.
//!
//! [`Kind`] is the domain and [`Value`] is the inhabitant. Everything a person
//! types arrives as text and leaves through [`Kind::parse`], which is what makes
//! a `set` a coercion into a domain rather than a blind string write — the trap
//! `crates/crcbl-cli/src/settings_cmd.rs` already warns about.

use std::fmt;

/// Why the console refused what it was handed.
///
/// One type for every refusal — a bad coercion, a value outside its range, a
/// read-only variable, an unterminated quote, a command's own complaint —
/// because every one of them has exactly the same fate: it is printed and the
/// state is left alone. A hand-written `Display` rather than a `thiserror`
/// derive, because this crate has no dependencies at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault(String);

impl Fault {
    /// A fault carrying `message`, which is the whole of what a person sees.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// The message, without the `Display` round-trip.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Fault {}

/// The domain of a console variable: what values it accepts and how text
/// coerces into one.
///
/// `PartialEq` but not `Eq`: [`Kind::Float`] holds floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `true`/`false`, and the spellings [`Kind::parse`] documents.
    Bool,
    /// A whole number, refused outside the inclusive range.
    Int {
        /// The smallest accepted value.
        min: i64,
        /// The largest accepted value.
        max: i64,
    },
    /// A finite `f32`, refused outside the inclusive range.
    Float {
        /// The smallest accepted value.
        min: f32,
        /// The largest accepted value.
        max: f32,
    },
    /// One of a fixed set of names, matched exactly and then case-insensitively.
    Enum(&'static [&'static str]),
    /// Free text.
    ///
    /// **No [`ConVar`](crate::ConVar) has this kind** — a `String` in a `static`
    /// needs a lock and an allocation the engine's statics do not want, so a
    /// text variable exists only as a [`Binding`](crate::Binding), whose storage
    /// is somewhere that can hold one. Plan decision 1.
    Text,
}

impl Kind {
    /// How the kind is named in a fault, e.g. `"an int"`.
    ///
    /// The article is part of it so the messages read as sentences rather than
    /// as a label with a word glued in front.
    #[must_use]
    pub const fn article_name(self) -> &'static str {
        match self {
            Self::Bool => "a bool",
            Self::Int { .. } => "an int",
            Self::Float { .. } => "a float",
            Self::Enum(_) => "an enum",
            Self::Text => "text",
        }
    }

    /// Coerce typed text into a [`Value`] of this kind.
    ///
    /// - [`Bool`](Self::Bool) accepts `true`/`false`, `1`/`0`, `on`/`off` and
    ///   `yes`/`no`, in any case.
    /// - [`Int`](Self::Int) and [`Float`](Self::Float) parse with the standard
    ///   library and are refused outside their range, with the range in the
    ///   fault. A float must be finite: `inf` and `NaN` are outside every range
    ///   and would compare `false` against both ends rather than one.
    /// - [`Enum`](Self::Enum) matches a value exactly first and then
    ///   case-insensitively, so a set holding two spellings that differ only in
    ///   case still resolves to the one that was typed.
    /// - [`Text`](Self::Text) accepts anything.
    ///
    /// # Errors
    ///
    /// A [`Fault`] naming what was typed and what the kind wanted.
    pub fn parse(self, text: &str) -> Result<Value, Fault> {
        match self {
            Self::Bool => match text.to_ascii_lowercase().as_str() {
                "true" | "1" | "on" | "yes" => Ok(Value::Bool(true)),
                "false" | "0" | "off" | "no" => Ok(Value::Bool(false)),
                _ => Err(Fault::new(format!(
                    "`{text}` is not a bool: try true/false, 1/0, on/off or yes/no"
                ))),
            },
            Self::Int { min, max } => {
                let parsed: i64 = text
                    .parse()
                    .map_err(|_| Fault::new(format!("`{text}` is not a whole number")))?;
                if parsed < min || parsed > max {
                    return Err(Fault::new(format!("{parsed} is outside {min}..={max}")));
                }
                Ok(Value::Int(parsed))
            }
            Self::Float { min, max } => {
                let parsed: f32 = text
                    .parse()
                    .map_err(|_| Fault::new(format!("`{text}` is not a number")))?;
                if !parsed.is_finite() || parsed < min || parsed > max {
                    return Err(Fault::new(format!("{parsed} is outside {min}..={max}")));
                }
                Ok(Value::Float(parsed))
            }
            Self::Enum(values) => {
                if let Some(exact) = values.iter().copied().find(|candidate| *candidate == text) {
                    return Ok(Value::Enum(exact));
                }
                let folded = text.to_ascii_lowercase();
                values
                    .iter()
                    .copied()
                    .find(|candidate| candidate.to_ascii_lowercase() == folded)
                    .map(Value::Enum)
                    .ok_or_else(|| {
                        Fault::new(format!("`{text}` is not one of: {}", values.join(", ")))
                    })
            }
            Self::Text => Ok(Value::Text(text.to_owned())),
        }
    }

    /// Whether `value` is one this kind admits, named against `name`.
    ///
    /// The boundary every `set` goes through, whether the value came from
    /// [`parse`](Self::parse) or was built by a caller: the range check lives
    /// here rather than only in the parser, so a hand-made
    /// `Value::Int(i64::MAX)` is refused the same way typed text is.
    ///
    /// # Errors
    ///
    /// A [`Fault`] naming `name`, the kind it is, and what it was handed.
    pub fn check(self, name: &str, value: &Value) -> Result<(), Fault> {
        match (self, value) {
            (Self::Bool, Value::Bool(_)) | (Self::Text, Value::Text(_)) => Ok(()),
            (Self::Int { min, max }, Value::Int(v)) => {
                if *v < min || *v > max {
                    return Err(Fault::new(format!(
                        "`{name}`: {v} is outside {min}..={max}"
                    )));
                }
                Ok(())
            }
            (Self::Float { min, max }, Value::Float(v)) => {
                if !v.is_finite() || *v < min || *v > max {
                    return Err(Fault::new(format!(
                        "`{name}`: {v} is outside {min}..={max}"
                    )));
                }
                Ok(())
            }
            (Self::Enum(values), Value::Enum(v)) => {
                if values.contains(v) {
                    return Ok(());
                }
                Err(Fault::new(format!(
                    "`{name}`: `{v}` is not one of: {}",
                    values.join(", ")
                )))
            }
            _ => Err(Fault::new(format!(
                "`{name}` is {}, not {}",
                self.article_name(),
                value.article_name()
            ))),
        }
    }
}

/// A console value: what a variable holds and what a person typed.
///
/// `PartialEq` but not `Eq`: [`Value::Float`] holds a float.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A [`Kind::Bool`] value.
    Bool(bool),
    /// A [`Kind::Int`] value.
    Int(i64),
    /// A [`Kind::Float`] value.
    Float(f32),
    /// One name from a [`Kind::Enum`]'s set, borrowed from that set.
    Enum(&'static str),
    /// A [`Kind::Text`] value.
    Text(String),
}

impl Value {
    /// How the value's own kind is named in a fault, e.g. `"an int"`.
    #[must_use]
    pub const fn article_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "a bool",
            Self::Int(_) => "an int",
            Self::Float(_) => "a float",
            Self::Enum(_) => "an enum",
            Self::Text(_) => "text",
        }
    }
}

/// Prints in the form a person types back.
///
/// Text is bare wherever bare text parses back to the same string, and quoted
/// otherwise — when it is empty, or holds whitespace or any character the line
/// parser would act on. Inside the quotes only `"` and `\` are escaped, which
/// are the only two [`parse_line`](crate::parse_line) reads.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Enum(v) => f.write_str(v),
            Self::Text(v) if !needs_quotes(v) => f.write_str(v),
            Self::Text(v) => {
                f.write_str("\"")?;
                for c in v.chars() {
                    match c {
                        '"' => f.write_str("\\\"")?,
                        '\\' => f.write_str("\\\\")?,
                        c => write!(f, "{c}")?,
                    }
                }
                f.write_str("\"")
            }
        }
    }
}

/// Whether printing `text` bare would parse back as something else.
fn needs_quotes(text: &str) -> bool {
    text.is_empty()
        || text
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | ';' | '='))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AA: Kind = Kind::Enum(&["none", "fxaa", "smaa"]);

    #[test]
    fn every_bool_spelling_parses_in_either_case() {
        for text in ["true", "TRUE", "1", "on", "ON", "yes", "Yes"] {
            assert_eq!(Kind::Bool.parse(text), Ok(Value::Bool(true)), "{text}");
        }
        for text in ["false", "False", "0", "off", "OFF", "no", "No"] {
            assert_eq!(Kind::Bool.parse(text), Ok(Value::Bool(false)), "{text}");
        }
    }

    #[test]
    fn a_bool_refusal_lists_the_spellings_it_wanted() {
        let fault = Kind::Bool.parse("maybe").expect_err("not a bool");
        assert_eq!(
            fault.message(),
            "`maybe` is not a bool: try true/false, 1/0, on/off or yes/no"
        );
    }

    #[test]
    fn an_int_parses_and_its_range_ends_are_inclusive() {
        let kind = Kind::Int { min: 1, max: 16 };
        assert_eq!(kind.parse("1"), Ok(Value::Int(1)));
        assert_eq!(kind.parse("16"), Ok(Value::Int(16)));
        assert_eq!(kind.parse("+7"), Ok(Value::Int(7)));
    }

    #[test]
    fn an_int_outside_its_range_is_refused_with_the_range_in_the_message() {
        let kind = Kind::Int { min: 1, max: 16 };
        assert_eq!(
            kind.parse("17").expect_err("above the range").message(),
            "17 is outside 1..=16"
        );
        assert_eq!(
            kind.parse("0").expect_err("below the range").message(),
            "0 is outside 1..=16"
        );
    }

    #[test]
    fn text_that_is_not_a_number_is_refused_by_the_numeric_kinds() {
        assert_eq!(
            Kind::Int { min: 0, max: 9 }
                .parse("seven")
                .expect_err("not a number")
                .message(),
            "`seven` is not a whole number"
        );
        assert_eq!(
            Kind::Float { min: 0.0, max: 9.0 }
                .parse("seven")
                .expect_err("not a number")
                .message(),
            "`seven` is not a number"
        );
    }

    #[test]
    fn a_float_accepts_both_range_ends_and_refuses_just_outside_them() {
        let kind = Kind::Float { min: 0.0, max: 1.0 };
        assert_eq!(kind.parse("0"), Ok(Value::Float(0.0)));
        assert_eq!(kind.parse("1"), Ok(Value::Float(1.0)));
        assert_eq!(kind.parse("0.75"), Ok(Value::Float(0.75)));
        assert_eq!(
            kind.parse("1.0001").expect_err("above the range").message(),
            "1.0001 is outside 0..=1"
        );
        assert_eq!(
            kind.parse("-0.0001")
                .expect_err("below the range")
                .message(),
            "-0.0001 is outside 0..=1"
        );
    }

    #[test]
    fn a_non_finite_float_is_outside_every_range() {
        let kind = Kind::Float {
            min: f32::MIN,
            max: f32::MAX,
        };
        for text in ["inf", "-inf", "NaN"] {
            let fault = kind.parse(text).expect_err("not finite");
            assert!(
                fault.message().contains("is outside"),
                "{text}: {}",
                fault.message()
            );
        }
    }

    #[test]
    fn an_enum_matches_exactly_and_then_case_insensitively() {
        assert_eq!(AA.parse("smaa"), Ok(Value::Enum("smaa")));
        assert_eq!(AA.parse("SMAA"), Ok(Value::Enum("smaa")));
        assert_eq!(AA.parse("Fxaa"), Ok(Value::Enum("fxaa")));
    }

    #[test]
    fn an_unknown_enum_name_is_refused_listing_the_set() {
        assert_eq!(
            AA.parse("taa").expect_err("not in the set").message(),
            "`taa` is not one of: none, fxaa, smaa"
        );
    }

    #[test]
    fn text_accepts_whatever_it_is_given() {
        assert_eq!(Kind::Text.parse(""), Ok(Value::Text(String::new())));
        assert_eq!(
            Kind::Text.parse("a b c"),
            Ok(Value::Text("a b c".to_owned()))
        );
    }

    #[test]
    fn check_refuses_a_value_of_another_kind_by_name() {
        assert_eq!(
            Kind::Bool
                .check("r_ao_view", &Value::Int(1))
                .expect_err("wrong kind")
                .message(),
            "`r_ao_view` is a bool, not an int"
        );
    }

    #[test]
    fn check_refuses_a_hand_built_value_outside_the_range() {
        let kind = Kind::Int { min: 1, max: 16 };
        assert_eq!(
            kind.check("anisotropy", &Value::Int(64))
                .expect_err("out of range")
                .message(),
            "`anisotropy`: 64 is outside 1..=16"
        );
        let kind = Kind::Float { min: 0.0, max: 1.0 };
        assert_eq!(
            kind.check("gain", &Value::Float(f32::NAN))
                .expect_err("not finite")
                .message(),
            "`gain`: NaN is outside 0..=1"
        );
    }

    #[test]
    fn check_refuses_an_enum_name_that_is_not_in_the_set() {
        assert_eq!(
            AA.check("antialiasing", &Value::Enum("taa"))
                .expect_err("not in the set")
                .message(),
            "`antialiasing`: `taa` is not one of: none, fxaa, smaa"
        );
        assert_eq!(AA.check("antialiasing", &Value::Enum("smaa")), Ok(()));
    }

    #[test]
    fn a_value_prints_in_the_form_it_is_typed_back() {
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::Int(3).to_string(), "3");
        assert_eq!(Value::Float(0.75).to_string(), "0.75");
        assert_eq!(Value::Enum("smaa").to_string(), "smaa");
        assert_eq!(Value::Text("plain".to_owned()).to_string(), "plain");
    }

    #[test]
    fn text_is_quoted_only_when_bare_text_would_parse_back_as_something_else() {
        assert_eq!(Value::Text(String::new()).to_string(), "\"\"");
        assert_eq!(
            Value::Text("two words".to_owned()).to_string(),
            "\"two words\""
        );
        assert_eq!(Value::Text("a;b".to_owned()).to_string(), "\"a;b\"");
        assert_eq!(Value::Text("a=b".to_owned()).to_string(), "\"a=b\"");
        assert_eq!(
            Value::Text("say \"hi\"".to_owned()).to_string(),
            "\"say \\\"hi\\\"\""
        );
        assert_eq!(
            Value::Text("back\\slash".to_owned()).to_string(),
            "\"back\\\\slash\""
        );
    }

    #[test]
    fn a_fault_displays_and_is_an_error() {
        let fault = Fault::new("nope");
        assert_eq!(fault.to_string(), "nope");
        assert_eq!(fault.message(), "nope");
        let as_error: &dyn std::error::Error = &fault;
        assert_eq!(as_error.to_string(), "nope");
    }
}
