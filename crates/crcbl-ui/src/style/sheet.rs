//! A stylesheet parsed from CSS text, and what went wrong on the way.
//!
//! `cssparser` tokenizes and splits the text into rules and declarations;
//! the selector, property and value modules give each piece its meaning.
//!
//! # Errors and warnings
//!
//! Parsing never stops at the first problem. CSS's own recovery — skip to the
//! end of the rule, or of the declaration — carries on past each one, so a
//! sheet reports every problem it has in one pass, and what did parse is
//! kept. Each problem is a [`Diagnostic`] with the file, line and column
//! cssparser located it at, in one of two severities:
//!
//! * **An error is text that is not a rule**: a selector outside the grammar,
//!   a stray token between rules, a declaration without a `:`. Whoever wrote it
//!   meant something the sheet does not say, so a reload that has one keeps
//!   the last good sheet — see [`crate::tree::Ui::replace_stylesheet`].
//! * **A warning is a rule that says something this subset does not do**: an
//!   unknown property, a value a property does not take, an at-rule. The rest
//!   of the rule applies, as a browser would apply it.

use std::fmt;
use std::sync::Arc;

use cssparser::{
    AtRuleParser, BasicParseErrorKind, CowRcStr, DeclarationParser, ParseError, ParseErrorKind,
    Parser, ParserState, QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, SourceLocation,
    StyleSheetParser,
};

use super::property::Property;
use super::selector::{Selector, parse_selector_list};
use super::value::Declaration;

/// How bad a [`Diagnostic`] is; see the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Something the subset ignores; the sheet is used.
    Warning,
    /// Text that is not CSS the subset reads; a reload keeps the last good
    /// sheet.
    Error,
}

/// One problem in a stylesheet, where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The sheet's name: its path, for a sheet loaded from one.
    pub file: Arc<str>,
    /// One-based.
    pub line: u32,
    /// One-based, as cssparser counts it.
    pub column: u32,
    /// Whether the sheet is still used.
    pub severity: Severity,
    /// What was wrong.
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(
            f,
            "{}:{}:{}: {severity}: {}",
            self.file, self.line, self.column, self.message
        )
    }
}

/// A CSS-wide keyword a declaration can give any property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WideKeyword {
    /// The property's initial value.
    Initial,
    /// Inherited for `color` and `font-size`, initial for the rest.
    Unset,
}

/// One declaration as a rule stores it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Decl {
    /// A typed value, known at parse time.
    Set(Declaration),
    /// `initial` or `unset`.
    Keyword(Property, WideKeyword),
    /// `--name: value`, kept as text.
    Custom(Arc<str>, Arc<str>),
    /// A value holding `var()`, parsed once its variables are known.
    Var {
        property: Property,
        /// The property as written, for the diagnostic.
        name: Arc<str>,
        css: Arc<str>,
        /// Where it was written, for the diagnostic.
        at: Location,
    },
}

/// A file and a position in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub file: Arc<str>,
    pub line: u32,
    pub column: u32,
}

/// One rule: a selector list and its declarations.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Arc<[Decl]>,
}

/// A parsed stylesheet: its rules in source order.
#[derive(Clone, Debug, PartialEq)]
pub struct Stylesheet {
    name: Arc<str>,
    pub(crate) rules: Vec<Rule>,
}

impl Stylesheet {
    /// Parses `css`, naming it `name` in every diagnostic.
    ///
    /// Always returns the rules that parsed, with every problem found; whether
    /// to use a sheet with errors is the caller's decision.
    #[must_use]
    pub fn parse(name: &str, css: &str) -> (Self, Vec<Diagnostic>) {
        let file: Arc<str> = Arc::from(name);
        let mut input = Parser::new(css);
        let mut parser = SheetParser {
            file: file.clone(),
            diagnostics: Vec::new(),
        };
        let mut rules = Vec::new();
        let mut results = Vec::new();
        for result in StyleSheetParser::new(&mut input, &mut parser) {
            results.push(result.map_err(|(error, slice, at)| (error, slice.to_owned(), at)));
        }
        for result in results {
            match result {
                Ok(rule) => rules.push(rule),
                Err((error, slice, at)) => {
                    let (severity, message) = describe(error, &slice);
                    parser.report(at, severity, message);
                }
            }
        }
        let mut diagnostics = parser.diagnostics;
        diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
        (Self { name: file, rules }, diagnostics)
    }

    /// The name diagnostics call it by.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How many rules parsed.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// What a parse step refused, beyond cssparser's own kinds.
#[derive(Debug)]
enum Refusal {
    /// A selector outside the grammar; the message says which part.
    Selector(String),
    /// An at-rule, which the subset has none of.
    AtRule(String),
    UnknownProperty(String),
    InvalidValue {
        property: String,
        value: String,
    },
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Self::Selector(message)
    }
}

/// A refusal's severity and message; `slice` is the source it covered.
fn describe(error: ParseError<Refusal>, slice: &str) -> (Severity, String) {
    let slice = slice.trim();
    match error.kind {
        ParseErrorKind::Custom(Refusal::Selector(message)) => (
            Severity::Error,
            format!("{message}; the rule `{}` is dropped", head(slice)),
        ),
        ParseErrorKind::Custom(Refusal::AtRule(name)) => (
            Severity::Warning,
            format!("the at-rule `@{name}` is not supported and is skipped"),
        ),
        ParseErrorKind::Custom(Refusal::UnknownProperty(name)) => {
            (Severity::Warning, format!("unknown property `{name}`"))
        }
        ParseErrorKind::Custom(Refusal::InvalidValue { property, value }) => (
            Severity::Warning,
            format!("`{value}` is not a value `{property}` takes"),
        ),
        ParseErrorKind::Basic(BasicParseErrorKind::EndOfInput) => (
            Severity::Error,
            format!("`{}` ends before it is complete", head(slice)),
        ),
        ParseErrorKind::Basic(kind) => (Severity::Error, format!("{kind} in `{}`", head(slice))),
    }
}

/// The first line of `slice`, for quoting in a message.
fn head(slice: &str) -> &str {
    slice.lines().next().unwrap_or("").trim()
}

struct SheetParser {
    file: Arc<str>,
    diagnostics: Vec<Diagnostic>,
}

impl SheetParser {
    fn report(&mut self, at: SourceLocation, severity: Severity, message: String) {
        self.diagnostics.push(Diagnostic {
            file: self.file.clone(),
            line: at.line + 1,
            column: at.column,
            severity,
            message,
        });
    }
}

impl<'i> QualifiedRuleParser<'i> for SheetParser {
    type Prelude = Vec<Selector>;
    type QualifiedRule = Rule;
    type Error = Refusal;

    fn parse_prelude(
        &mut self,
        input: &mut Parser<'i>,
    ) -> Result<Self::Prelude, ParseError<Self::Error>> {
        parse_selector_list(input).map_err(|error| ParseError {
            kind: error.kind.into(),
        })
    }

    fn parse_block(
        &mut self,
        selectors: Self::Prelude,
        _start: &ParserState,
        input: &mut Parser<'i>,
    ) -> Result<Self::QualifiedRule, ParseError<Self::Error>> {
        let mut body = BodyParser {
            file: self.file.clone(),
        };
        let mut declarations = Vec::new();
        let mut problems = Vec::new();
        for result in RuleBodyParser::new(input, &mut body) {
            match result {
                Ok(parsed) => declarations.extend(parsed),
                Err((error, slice, at)) => problems.push((error, slice.to_owned(), at)),
            }
        }
        for (error, slice, at) in problems {
            let (severity, message) = describe(error, &slice);
            self.report(at, severity, message);
        }
        Ok(Rule {
            selectors,
            declarations: declarations.into(),
        })
    }
}

impl<'i> AtRuleParser<'i> for SheetParser {
    type Prelude = ();
    type AtRule = Rule;
    type Error = Refusal;

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        _input: &mut Parser<'i>,
    ) -> Result<Self::Prelude, ParseError<Self::Error>> {
        Err(ParseError::custom(Refusal::AtRule(name.to_string())))
    }
}

/// Parses one rule's declarations.
struct BodyParser {
    file: Arc<str>,
}

impl<'i> DeclarationParser<'i> for BodyParser {
    type Declaration = Vec<Decl>;
    type Error = Refusal;

    fn parse_value(
        &mut self,
        name: CowRcStr<'i>,
        input: &mut Parser<'i>,
        start: &ParserState,
    ) -> Result<Self::Declaration, ParseError<Self::Error>> {
        let value_start = input.state();
        input.look_for_arbitrary_substitution_functions(&["var"]);
        while input.next_including_whitespace_and_comments().is_ok() {}
        let uses_var = input.seen_arbitrary_substitution_functions();
        let css = input.slice_from(value_start.position()).trim();

        if name.starts_with("--") {
            return Ok(vec![Decl::Custom(Arc::from(&*name), Arc::from(css))]);
        }
        let Some(property) = Property::from_name(&name) else {
            return Err(ParseError::custom(Refusal::UnknownProperty(
                name.to_string(),
            )));
        };
        if uses_var {
            let at = start.source_location();
            return Ok(vec![Decl::Var {
                property,
                name: Arc::from(&*name),
                css: Arc::from(css),
                at: Location {
                    file: self.file.clone(),
                    line: at.line + 1,
                    column: at.column,
                },
            }]);
        }

        input.reset(&value_start);
        let keyword = input.try_parse(|input| {
            let ident = input.expect_ident()?.clone();
            let keyword = cssparser::match_ignore_ascii_case! { &ident,
                "initial" => WideKeyword::Initial,
                "unset" => WideKeyword::Unset,
                _ => return Err(ParseError::custom(())),
            };
            input.expect_exhausted()?;
            Ok::<_, ParseError<()>>(keyword)
        });
        if let Ok(keyword) = keyword {
            return Ok(vec![Decl::Keyword(property, keyword)]);
        }
        let mut parsed = Vec::new();
        property.parse(input, &mut parsed).map_err(|_| {
            ParseError::custom(Refusal::InvalidValue {
                property: name.to_string(),
                value: css.to_owned(),
            })
        })?;
        Ok(parsed.into_iter().map(Decl::Set).collect())
    }
}

impl<'i> AtRuleParser<'i> for BodyParser {
    type Prelude = ();
    type AtRule = Vec<Decl>;
    type Error = Refusal;

    fn parse_prelude(
        &mut self,
        name: CowRcStr<'i>,
        _input: &mut Parser<'i>,
    ) -> Result<Self::Prelude, ParseError<Self::Error>> {
        Err(ParseError::custom(Refusal::AtRule(name.to_string())))
    }
}

impl QualifiedRuleParser<'_> for BodyParser {
    type Prelude = ();
    type QualifiedRule = Vec<Decl>;
    type Error = Refusal;
}

impl<'i> RuleBodyItemParser<'i, Vec<Decl>, Refusal> for BodyParser {
    fn parse_declarations(&self) -> bool {
        true
    }

    /// Nested rules are not in the subset.
    fn parse_qualified(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::LengthAuto;

    fn errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
        diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect()
    }

    fn set(rule: &Rule) -> Vec<Declaration> {
        rule.declarations
            .iter()
            .map(|decl| match decl {
                Decl::Set(declaration) => *declaration,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    /// **Comments anywhere, escapes in names and CRLF line endings parse** into
    /// the rules they spell.
    #[test]
    fn comments_escapes_and_line_endings_parse() {
        let css = "/* head */ .a/* x */{ /* in */ width /* y */: /* z */ 10px /* w */; }\r\n\
                   #b\\:c { height: 2px }\r\n/* tail, unterminated";
        let (sheet, diagnostics) = Stylesheet::parse("t.css", css);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(sheet.rule_count(), 2);
        assert_eq!(
            set(&sheet.rules[0]),
            [Declaration::Width(LengthAuto::Px(10.0))]
        );
        assert_eq!(
            sheet.rules[1].selectors[0].subject.id.as_deref(),
            Some("b:c")
        );
    }

    /// **A bad rule is skipped to the end of its block and every rule after it
    /// still parses** — and each problem is reported at its own line and column.
    #[test]
    fn a_bad_rule_recovers_to_the_next_and_every_problem_is_located() {
        let css = "\
.ok { width: 1px }
.bad + .sibling { width: 2px }
.also-ok { height: 3px; wobble: 4px; width: nope; height: 5px }
@media screen { .inside { width: 6px } }
} stray
.last { width: 7px; }
.missing-colon { width 8px; height: 9px }
";
        let (sheet, diagnostics) = Stylesheet::parse("theme.css", css);
        let located: Vec<(u32, u32, Severity)> = diagnostics
            .iter()
            .map(|d| (d.line, d.column, d.severity))
            .collect();
        assert_eq!(
            located,
            [
                (2, 1, Severity::Error),
                (3, 25, Severity::Warning),
                (3, 38, Severity::Warning),
                (4, 1, Severity::Warning),
                (5, 1, Severity::Error),
                (7, 18, Severity::Error),
            ],
            "{diagnostics:#?}"
        );
        assert!(
            diagnostics[0].message.contains("sibling"),
            "{}",
            diagnostics[0]
        );
        assert!(
            diagnostics[1].message.contains("`wobble`"),
            "{}",
            diagnostics[1]
        );
        assert!(
            diagnostics[2].message.contains("`nope`"),
            "{}",
            diagnostics[2]
        );
        assert!(
            diagnostics[3].message.contains("@media"),
            "{}",
            diagnostics[3]
        );
        assert_eq!(
            diagnostics[0].to_string().split(": ").next(),
            Some("theme.css:2:1")
        );

        let names: Vec<_> = sheet
            .rules
            .iter()
            .map(|rule| rule.selectors[0].subject.classes[0].to_string())
            .collect();
        // `} stray` is the prelude of a rule whose block is `.last`'s, as CSS
        // reads it, so `.last` goes with it and the rule after still parses.
        assert_eq!(names, ["ok", "also-ok", "missing-colon"]);
        assert_eq!(
            set(&sheet.rules[1]),
            [
                Declaration::Height(LengthAuto::Px(3.0)),
                Declaration::Height(LengthAuto::Px(5.0))
            ],
            "the good declarations around the two bad ones were lost"
        );
        assert_eq!(
            set(&sheet.rules[2]),
            [Declaration::Height(LengthAuto::Px(9.0))]
        );
    }

    /// **Custom properties keep their text, `var()` values wait for it, and
    /// `initial` and `unset` are keywords** rather than values.
    #[test]
    fn custom_properties_var_values_and_wide_keywords_are_kept_unresolved() {
        let css =
            ".a { --gap:  4px 2px ; padding: var(--gap); width: initial; color: UNSET; --empty:; }";
        let (sheet, diagnostics) = Stylesheet::parse("vars.css", css);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let decls = &sheet.rules[0].declarations;
        assert_eq!(
            decls[0],
            Decl::Custom(Arc::from("--gap"), Arc::from("4px 2px"))
        );
        assert!(
            matches!(&decls[1], Decl::Var { property: Property::Padding(_), css, at, .. }
                if &**css == "var(--gap)" && at.line == 1 && at.column == 24),
            "{:?}",
            decls[1]
        );
        assert_eq!(
            decls[2],
            Decl::Keyword(Property::Width, WideKeyword::Initial)
        );
        assert_eq!(decls[3], Decl::Keyword(Property::Color, WideKeyword::Unset));
        assert_eq!(decls[4], Decl::Custom(Arc::from("--empty"), Arc::from("")));
    }

    /// A sheet cut off mid-rule parses what it has, as CSS closes an
    /// unterminated block at the end of the file.
    #[test]
    fn a_truncated_sheet_keeps_what_it_has() {
        let (sheet, diagnostics) = Stylesheet::parse("cut.css", ".a { width: 1px; height:");
        assert_eq!(errors(&diagnostics), Vec::<&Diagnostic>::new());
        assert_eq!(
            set(&sheet.rules[0]),
            [Declaration::Width(LengthAuto::Px(1.0))]
        );
    }
}
