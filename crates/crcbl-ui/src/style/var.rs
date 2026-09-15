//! Custom properties and `var()`: substituting one into a value, and resolving
//! a node's custom properties through each other.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use cssparser::{ParseError, Parser, Token};

/// The custom properties one node sees, name to value, each already free of
/// `var()`.
pub(crate) type CustomProperties = BTreeMap<Arc<str>, Arc<str>>;

/// `css` with every `var(--name, fallback)` replaced by the value `lookup`
/// gives for `--name`, or by its substituted fallback when there is none.
///
/// `None` when a `var()` has neither, or is malformed: the declaration is then
/// invalid at computed-value time.
pub(crate) fn substitute(
    css: &str,
    lookup: &mut impl FnMut(&str) -> Option<Arc<str>>,
) -> Option<String> {
    let mut out = String::with_capacity(css.len());
    let mut input = Parser::new(css);
    substitute_into(&mut input, lookup, &mut out).ok()?;
    Some(out)
}

fn substitute_into<'i>(
    input: &mut Parser<'i>,
    lookup: &mut impl FnMut(&str) -> Option<Arc<str>>,
    out: &mut String,
) -> Result<(), ParseError<()>> {
    loop {
        let start = input.position();
        let Ok(token) = input.next_including_whitespace_and_comments() else {
            return Ok(());
        };
        let closing = match *token {
            Token::Function(ref name) if name.eq_ignore_ascii_case("var") => {
                input.parse_nested_block(|input| {
                    let name = input.expect_ident()?.clone();
                    if !name.starts_with("--") {
                        return Err(ParseError::custom(()));
                    }
                    if let Some(value) = lookup(&name) {
                        // A fallback is still parsed, so a malformed one is still
                        // an error, but its substitution is thrown away.
                        if input.try_parse(|input| input.expect_comma()).is_ok() {
                            substitute_into(input, lookup, &mut String::new())?;
                        }
                        out.push_str(&value);
                        return Ok(());
                    }
                    input.expect_comma()?;
                    substitute_into(input, lookup, out)
                })?;
                continue;
            }
            Token::Function(_) | Token::ParenthesisBlock => ')',
            Token::SquareBracketBlock => ']',
            Token::CurlyBracketBlock => '}',
            _ => {
                out.push_str(input.slice_from(start));
                continue;
            }
        };
        out.push_str(input.slice_from(start));
        input.parse_nested_block(|input| substitute_into(input, lookup, out))?;
        out.push(closing);
    }
}

/// The custom properties a node sees: `inherited`, overridden by its `own`,
/// each with `var()` substituted — and every property on a reference cycle
/// removed, as the specification makes one invalid at computed-value time
/// whatever fallback it names.
pub(crate) fn resolve_custom(
    inherited: &CustomProperties,
    own: BTreeMap<Arc<str>, Arc<str>>,
) -> CustomProperties {
    #[derive(Clone)]
    enum State {
        InProgress,
        Done(Option<Arc<str>>),
    }

    struct Walk<'a> {
        inherited: &'a CustomProperties,
        own: &'a BTreeMap<Arc<str>, Arc<str>>,
        states: HashMap<Arc<str>, State>,
        /// The properties being resolved, outermost first.
        stack: Vec<Arc<str>>,
        cyclic: Vec<Arc<str>>,
    }

    impl Walk<'_> {
        fn resolve(&mut self, name: &str) -> Option<Arc<str>> {
            let Some((key, raw)) = self.own.get_key_value(name) else {
                return self.inherited.get(name).cloned();
            };
            match self.states.get(name) {
                Some(State::Done(value)) => return value.clone(),
                Some(State::InProgress) => {
                    // Everything from `name` to the top of the stack is on the cycle.
                    let from = self
                        .stack
                        .iter()
                        .position(|entry| &**entry == name)
                        .unwrap_or(0);
                    self.cyclic.extend(self.stack[from..].iter().cloned());
                    return None;
                }
                None => {}
            }
            let (key, raw) = (key.clone(), raw.clone());
            self.states.insert(key.clone(), State::InProgress);
            self.stack.push(key.clone());
            let substituted = substitute(&raw, &mut |var| self.resolve(var));
            self.stack.pop();
            let value = if self.cyclic.contains(&key) {
                None
            } else {
                substituted.map(|text| Arc::from(text.as_str()))
            };
            self.states.insert(key, State::Done(value.clone()));
            value
        }
    }

    let mut walk = Walk {
        inherited,
        own: &own,
        states: HashMap::new(),
        stack: Vec::new(),
        cyclic: Vec::new(),
    };
    let mut resolved = inherited.clone();
    for name in own.keys() {
        match walk.resolve(name) {
            Some(value) => {
                resolved.insert(name.clone(), value);
            }
            None => {
                resolved.remove(name);
            }
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl FnMut(&str) -> Option<Arc<str>> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| Arc::from(*value))
        }
    }

    /// **`var()` substitutes, falls back, nests inside functions and inside
    /// its own fallback, and a missing one with no fallback is invalid.**
    #[test]
    fn var_substitutes_and_falls_back() {
        let table = [("--a", "4px"), ("--red", "255")];
        let cases = [
            ("var(--a)", Some("4px")),
            ("var( --a )", Some("4px")),
            ("1px var(--a) 2px", Some("1px 4px 2px")),
            ("var(--missing, 9px)", Some(" 9px")),
            ("var(--missing,var(--a))", Some("4px")),
            ("var(--missing, var(--also-missing, 1px))", Some("  1px")),
            ("var(--a, garbage)", Some("4px")),
            ("rgb(var(--red), 0, 0)", Some("rgb(255, 0, 0)")),
            ("var(--missing)", None),
            ("var(a)", None),
            ("var(--missing, var(--also-missing))", None),
        ];
        for (css, want) in cases {
            assert_eq!(substitute(css, &mut vars(&table)).as_deref(), want, "{css}");
        }
    }

    fn custom(pairs: &[(&str, &str)]) -> BTreeMap<Arc<str>, Arc<str>> {
        pairs
            .iter()
            .map(|(name, value)| (Arc::from(*name), Arc::from(*value)))
            .collect()
    }

    /// **Custom properties resolve through each other and through what is
    /// inherited, and every property on a cycle is invalid** — including one
    /// whose `var()` names a fallback, and not a property that merely refers
    /// to the cycle with a fallback of its own.
    #[test]
    fn custom_properties_resolve_and_a_cycle_invalidates_every_member() {
        let inherited = custom(&[("--base", "3px"), ("--shadowed", "old")]);
        let own = custom(&[
            ("--shadowed", "new"),
            ("--double", "var(--base) var(--base)"),
            ("--chain", "var(--double)"),
            ("--a", "var(--b, red)"),
            ("--b", "var(--a)"),
            ("--self", "var(--self, 1px)"),
            ("--outside", "var(--a, blue)"),
            ("--dangling", "var(--nothing)"),
        ]);
        let resolved = resolve_custom(&inherited, own);
        let get = |name: &str| resolved.get(name).map(|value| value.to_string());
        assert_eq!(get("--base").as_deref(), Some("3px"));
        assert_eq!(get("--shadowed").as_deref(), Some("new"));
        assert_eq!(get("--double").as_deref(), Some("3px 3px"));
        assert_eq!(get("--chain").as_deref(), Some("3px 3px"));
        assert_eq!(
            get("--a"),
            None,
            "a cycle member with a fallback stayed valid"
        );
        assert_eq!(get("--b"), None);
        assert_eq!(get("--self"), None, "a self-reference stayed valid");
        assert_eq!(
            get("--outside").as_deref(),
            Some(" blue"),
            "a reference into a cycle is not on it"
        );
        assert_eq!(get("--dangling"), None);
    }
}
