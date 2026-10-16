//! CSS as SVG icons use it: declarations in a `style` attribute, and a
//! `<style>` sheet of rules whose selectors are simple.
//!
//! A selector here is one compound selector: an optional type (or `*`) and
//! any number of `.class` and `#id` parts, in a comma-separated list. That is
//! what icon themes write — breeze's `.ColorScheme-Text` above all — and a
//! combinator, an attribute selector, a pseudo-class or an at-rule is
//! refused rather than matched wrongly or not at all.

use super::xml::{Element, Namespace};
use crate::ImageError;

fn unsupported(what: impl core::fmt::Display) -> ImageError {
    ImageError::Unsupported(format!("SVG CSS: {what}"))
}

fn malformed(what: impl core::fmt::Display) -> ImageError {
    ImageError::Malformed(format!("SVG CSS: {what}"))
}

/// One `name: value` declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Declaration {
    /// The property name, lowercased.
    pub name: String,
    pub value: String,
    pub important: bool,
}

/// One compound selector.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Selector {
    /// The element name it requires, if any.
    element: Option<String>,
    ids: Vec<String>,
    classes: Vec<String>,
}

impl Selector {
    /// CSS specificity: ids, then classes, then types.
    pub fn specificity(&self) -> (usize, usize, usize) {
        (
            self.ids.len(),
            self.classes.len(),
            usize::from(self.element.is_some()),
        )
    }

    pub fn matches(&self, element: &Element) -> bool {
        if element.namespace != Namespace::Svg {
            return false;
        }
        if let Some(name) = &self.element
            && *name != element.name
        {
            return false;
        }
        let id = element.attribute(Namespace::None, "id");
        if !self.ids.iter().all(|want| id == Some(want.as_str())) {
            return false;
        }
        let classes = element.attribute(Namespace::None, "class").unwrap_or("");
        self.classes
            .iter()
            .all(|want| classes.split_ascii_whitespace().any(|class| class == want))
    }
}

/// One rule of a sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Rule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
}

/// `text` without its `/* comments */`.
fn strip_comments(text: &str) -> Result<String, ImageError> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("*/") else {
            return Err(malformed("a comment never ends"));
        };
        out.push(' ');
        rest = &rest[start + 2 + end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Splits `text` at each `separator` that is outside quotes and parentheses.
fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, c) if c == separator && depth == 0 => {
                parts.push(&text[start..index]);
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// The declarations of a `style` attribute or a rule's block.
///
/// # Errors
///
/// [`ImageError::Malformed`] for a declaration with no colon or an unclosed
/// comment.
pub(super) fn declarations(text: &str) -> Result<Vec<Declaration>, ImageError> {
    let text = strip_comments(text)?;
    let mut out = Vec::new();
    for part in split_top_level(&text, ';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((name, value)) = part.split_once(':') else {
            return Err(malformed(format_args!("the declaration {part:?}")));
        };
        let mut value = value.trim();
        let mut important = false;
        if let Some(at) = value.rfind('!')
            && value[at + 1..].trim().eq_ignore_ascii_case("important")
        {
            important = true;
            value = value[..at].trim_end();
        }
        out.push(Declaration {
            name: name.trim().to_ascii_lowercase(),
            value: value.to_owned(),
            important,
        });
    }
    Ok(out)
}

/// The rules of a `<style>` sheet, one per selector of each rule.
///
/// # Errors
///
/// [`ImageError::Unsupported`] for an at-rule or a selector this module does
/// not match, and [`ImageError::Malformed`] for a sheet that does not parse.
pub(super) fn sheet(text: &str) -> Result<Vec<Rule>, ImageError> {
    let text = strip_comments(text)?;
    // The HTML comment tokens a sheet may be wrapped in are whitespace to CSS.
    let text = text.replace("<!--", " ").replace("-->", " ");
    let mut rules = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        if rest.starts_with('@') {
            let name: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '{' && *c != ';')
                .collect();
            return Err(unsupported(format_args!("the at-rule {name}")));
        }
        let Some(open) = rest.find('{') else {
            return Err(malformed(format_args!("{rest:?} has no block")));
        };
        let Some(close) = rest[open..].find('}') else {
            return Err(malformed("a block never ends"));
        };
        let prelude = &rest[..open];
        let block = &rest[open + 1..open + close];
        let declarations = declarations(block)?;
        for text in prelude.split(',') {
            rules.push(Rule {
                selector: selector(text.trim())?,
                declarations: declarations.clone(),
            });
        }
        rest = rest[open + close + 1..].trim_start();
    }
    Ok(rules)
}

/// One compound selector.
fn selector(text: &str) -> Result<Selector, ImageError> {
    let refuse = || unsupported(format_args!("the selector {text:?}"));
    if text.is_empty() {
        return Err(malformed("an empty selector"));
    }
    let mut selector = Selector::default();
    let ident_length = |text: &str| {
        text.char_indices()
            .find(|&(_, c)| !(c.is_alphanumeric() || c == '-' || c == '_'))
            .map_or(text.len(), |(index, _)| index)
    };
    let mut rest = text;
    if let Some(after) = rest.strip_prefix('*') {
        rest = after;
    } else {
        let length = ident_length(rest);
        if length > 0 {
            selector.element = Some(rest[..length].to_owned());
            rest = &rest[length..];
        }
    }
    while let Some(marker) = rest.chars().next() {
        let after = &rest[1..];
        let length = ident_length(after);
        if length == 0 {
            return Err(refuse());
        }
        let name = after[..length].to_owned();
        match marker {
            '.' => selector.classes.push(name),
            '#' => selector.ids.push(name),
            _ => return Err(refuse()),
        }
        rest = &after[length..];
    }
    Ok(selector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::xml;

    fn declaration(name: &str, value: &str, important: bool) -> Declaration {
        Declaration {
            name: name.to_owned(),
            value: value.to_owned(),
            important,
        }
    }

    /// **A `style` attribute splits into its declarations**, comments
    /// dropped, `!important` noted, and a semicolon inside parentheses kept.
    #[test]
    fn declarations_split_at_top_level_semicolons() {
        assert_eq!(
            declarations("fill:red; /* c */ Stroke : url(#a;b) ;opacity:.5 !important;;")
                .expect("parses"),
            vec![
                declaration("fill", "red", false),
                declaration("stroke", "url(#a;b)", false),
                declaration("opacity", ".5", true),
            ]
        );
        assert!(matches!(
            declarations("fill red"),
            Err(ImageError::Malformed(_))
        ));
    }

    /// **A sheet's simple selectors match the elements they name**: by type,
    /// class, id and their combinations, with CSS's specificity.
    #[test]
    fn simple_selectors_match_and_rank() {
        let rules = sheet(
            "<!-- .ColorScheme-Text { color:#232629; } --> rect.a.b, #x { fill: red } * { opacity: 1 }",
        )
        .expect("parses");
        assert_eq!(rules.len(), 4);
        let document = xml::parse(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><path class="ColorScheme-Text"/><rect class="b a c"/><rect class="a"/><circle id="x"/></svg>"#,
        )
        .expect("parses");
        let matching = |index: usize| -> Vec<usize> {
            rules
                .iter()
                .enumerate()
                .filter(|(_, rule)| rule.selector.matches(&document.elements[index]))
                .map(|(at, _)| at)
                .collect()
        };
        assert_eq!(matching(1), [0, 3]);
        assert_eq!(matching(2), [1, 3]);
        assert_eq!(matching(3), [3]);
        assert_eq!(matching(4), [2, 3]);
        assert_eq!(rules[1].selector.specificity(), (0, 2, 1));
        assert_eq!(rules[2].selector.specificity(), (1, 0, 0));
        assert_eq!(rules[3].selector.specificity(), (0, 0, 0));
    }

    /// **What these selectors cannot express is refused**, not matched
    /// loosely: combinators, attributes, pseudo-classes and at-rules.
    #[test]
    fn complex_selectors_and_at_rules_are_refused() {
        for text in [
            "g rect { fill: red }",
            "g > rect { fill: red }",
            "rect[x] { fill: red }",
            "rect:hover { fill: red }",
            "a + b { fill: red }",
            "@media screen { rect { fill: red } }",
            "@import url(x.css);",
        ] {
            assert!(
                matches!(sheet(text), Err(ImageError::Unsupported(_))),
                "{text}"
            );
        }
        assert!(matches!(
            sheet("rect { fill: red"),
            Err(ImageError::Malformed(_))
        ));
        assert!(matches!(sheet("rect"), Err(ImageError::Malformed(_))));
    }
}
