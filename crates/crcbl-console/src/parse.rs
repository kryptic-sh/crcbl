//! Turning a typed line into the statements the registry runs.
//!
//! Source's grammar, which is what plan decision 7 asks for: a bare name, a name
//! and its arguments, an optional `=` between the two, double-quoted strings,
//! and `;` separating several statements on one line.

use crate::value::Fault;

/// One statement: a name and the arguments after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// The variable or command name, as typed.
    pub name: String,
    /// Everything after it, one token per argument, unquoted.
    pub args: Vec<String>,
}

/// A parsed line: the statements its `;` separated, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    statements: Vec<Statement>,
}

impl Line {
    /// The statements, in the order they were typed.
    #[must_use]
    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }

    /// Whether the line held nothing to run.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }
}

/// Parse a typed line.
///
/// - `name` is a statement with no arguments.
/// - `name value…` is a statement with them.
/// - `name = value` drops the `=`; so does `name=value`, which splits the first
///   token at its first `=`. Whitespace either side of the `=` is optional
///   because a person types all three forms and means one thing.
/// - `"a b"` is one argument. Inside the quotes, `\"` is a quote and `\\` is a
///   backslash; every other `\` stands for itself, so a Windows path is not an
///   escape puzzle.
/// - `;` ends a statement. It is ordinary text inside quotes.
///
/// Empty statements are dropped, so a trailing `;` and a blank line both parse
/// to a [`Line`] with nothing in it.
///
/// # Errors
///
/// A [`Fault`] when a quoted string is never closed, which is the one thing a
/// line can get wrong before anything is looked up.
pub fn parse_line(text: &str) -> Result<Line, Fault> {
    let mut statements = Vec::new();
    // Each token carries whether it arrived quoted: only a bare one takes part
    // in the `=` rule, so `echo "=b"` echoes `=b`.
    let mut tokens: Vec<(String, bool)> = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            ';' => finish(&mut statements, &mut tokens),
            '"' => {
                let mut token = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => match chars.next() {
                            Some(escaped @ ('"' | '\\')) => token.push(escaped),
                            Some(other) => {
                                token.push('\\');
                                token.push(other);
                            }
                            None => break,
                        },
                        c => token.push(c),
                    }
                }
                if !closed {
                    return Err(Fault::new("unterminated quoted string"));
                }
                tokens.push((token, true));
            }
            c if c.is_whitespace() => {}
            c => {
                let mut token = String::from(c);
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == ';' {
                        break;
                    }
                    token.push(c);
                    chars.next();
                }
                tokens.push((token, false));
            }
        }
    }
    finish(&mut statements, &mut tokens);

    Ok(Line { statements })
}

/// Turn the tokens gathered so far into a statement, if there are any.
///
/// This is where the optional `=` is dropped: after the name, and only there, so
/// `echo a = b` still echoes three arguments.
fn finish(statements: &mut Vec<Statement>, tokens: &mut Vec<(String, bool)>) {
    if tokens.is_empty() {
        return;
    }
    let mut args = tokens.split_off(1);
    let (mut name, name_quoted) = tokens.pop().expect("a statement has at least its name");

    // `name=value`, then `name =value` and `name = value`: the `=` is dropped
    // wherever it sits between the name and the first value.
    let split = (!name_quoted)
        .then(|| name.split_once('='))
        .flatten()
        .map(|(head, tail)| (head.to_owned(), tail.to_owned()));
    if let Some((head, tail)) = split {
        name = head;
        if !tail.is_empty() {
            args.insert(0, (tail, false));
        }
    }
    let stripped = args
        .first()
        .filter(|(_, quoted)| !quoted)
        .and_then(|(arg, _)| arg.strip_prefix('='))
        .map(str::to_owned);
    if let Some(rest) = stripped {
        if rest.is_empty() {
            args.remove(0);
        } else {
            args[0].0 = rest;
        }
    }

    if !name.is_empty() {
        statements.push(Statement {
            name,
            args: args.into_iter().map(|(arg, _)| arg).collect(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Vec<(String, Vec<String>)> {
        parse_line(text)
            .expect("parses")
            .statements()
            .iter()
            .map(|statement| (statement.name.clone(), statement.args.clone()))
            .collect()
    }

    fn one(text: &str) -> (String, Vec<String>) {
        let mut statements = parsed(text);
        assert_eq!(statements.len(), 1, "{text}");
        statements.remove(0)
    }

    #[test]
    fn a_bare_name_is_a_statement_with_no_arguments() {
        assert_eq!(one("antialiasing"), ("antialiasing".to_owned(), vec![]));
        assert_eq!(one("  antialiasing  "), ("antialiasing".to_owned(), vec![]));
    }

    #[test]
    fn a_name_and_its_arguments_keep_their_order() {
        assert_eq!(
            one("debug_view ambient occlusion"),
            (
                "debug_view".to_owned(),
                vec!["ambient".to_owned(), "occlusion".to_owned()]
            )
        );
    }

    #[test]
    fn the_equals_sign_is_optional_and_dropped_however_it_is_spaced() {
        let expected = ("antialiasing".to_owned(), vec!["smaa".to_owned()]);
        assert_eq!(one("antialiasing smaa"), expected);
        assert_eq!(one("antialiasing = smaa"), expected);
        assert_eq!(one("antialiasing =smaa"), expected);
        assert_eq!(one("antialiasing= smaa"), expected);
        assert_eq!(one("antialiasing=smaa"), expected);
    }

    #[test]
    fn a_quoted_first_argument_keeps_its_leading_equals_sign() {
        assert_eq!(
            one("echo \"=b\""),
            ("echo".to_owned(), vec!["=b".to_owned()])
        );
    }

    #[test]
    fn an_equals_sign_after_the_first_argument_is_ordinary_text() {
        assert_eq!(
            one("echo a = b"),
            (
                "echo".to_owned(),
                vec!["a".to_owned(), "=".to_owned(), "b".to_owned()]
            )
        );
    }

    #[test]
    fn a_quoted_string_is_one_argument_and_keeps_its_spaces() {
        assert_eq!(
            one("echo \"two words\" three"),
            (
                "echo".to_owned(),
                vec!["two words".to_owned(), "three".to_owned()]
            )
        );
        assert_eq!(one("echo \"\""), ("echo".to_owned(), vec![String::new()]));
    }

    #[test]
    fn the_two_escapes_inside_a_quoted_string_are_the_quote_and_the_backslash() {
        assert_eq!(
            one("echo \"say \\\"hi\\\"\""),
            ("echo".to_owned(), vec!["say \"hi\"".to_owned()])
        );
        assert_eq!(
            one("echo \"a\\\\b\""),
            ("echo".to_owned(), vec!["a\\b".to_owned()])
        );
        // Any other backslash stands for itself.
        assert_eq!(
            one("echo \"c:\\games\""),
            ("echo".to_owned(), vec!["c:\\games".to_owned()])
        );
    }

    #[test]
    fn a_semicolon_inside_quotes_is_text_and_outside_them_is_a_separator() {
        assert_eq!(
            one("echo \"a;b\""),
            ("echo".to_owned(), vec!["a;b".to_owned()])
        );
        assert_eq!(
            parsed("r_ao_view 1; antialiasing smaa; echo done"),
            vec![
                ("r_ao_view".to_owned(), vec!["1".to_owned()]),
                ("antialiasing".to_owned(), vec!["smaa".to_owned()]),
                ("echo".to_owned(), vec!["done".to_owned()]),
            ]
        );
    }

    #[test]
    fn empty_statements_are_dropped_rather_than_run() {
        assert!(parse_line("").expect("parses").is_empty());
        assert!(parse_line("   ").expect("parses").is_empty());
        assert!(parse_line(";;;").expect("parses").is_empty());
        assert_eq!(parsed("echo a;;").len(), 1);
    }

    #[test]
    fn an_unterminated_quote_is_refused_before_anything_is_looked_up() {
        assert_eq!(
            parse_line("echo \"never closed")
                .expect_err("unterminated")
                .message(),
            "unterminated quoted string"
        );
        assert_eq!(
            parse_line("echo \"trailing escape \\")
                .expect_err("unterminated")
                .message(),
            "unterminated quoted string"
        );
    }

    #[test]
    fn a_line_that_is_only_an_equals_sign_names_nothing() {
        assert!(parse_line("=").expect("parses").is_empty());
        assert!(parse_line("= value").expect("parses").is_empty());
    }
}
