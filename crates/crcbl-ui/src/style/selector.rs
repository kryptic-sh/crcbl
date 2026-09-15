//! Selectors: which nodes a rule applies to.
//!
//! The grammar is `docs/plan/07-ui-debug.md` section 3's, and no more:
//!
//! ```text
//! selector-list  = selector ("," selector)*
//! selector       = compound ((" " | ">") compound)*
//! compound       = (type | "*")? ("#" id | "." class | ":" pseudo-class)*
//! pseudo-class   = hover | active | focus | disabled | engaged
//! ```
//!
//! A type is `block`, `span` or a widget name a node declares; type names and
//! pseudo-classes are ASCII-case-insensitive, ids and classes are not, as in
//! CSS. Sibling combinators, attribute selectors, pseudo-elements and every
//! other pseudo-class are refused — the whole rule is dropped with an error
//! naming what was not understood, rather than a selector quietly matching
//! something narrower or wider than it says.
//!
//! # Specificity
//!
//! Simplified, and predictable over spec-faithful: a selector's [`Specificity`]
//! is the highest tier of anything in it — an id anywhere makes it an id
//! selector, else a class or pseudo-class makes it a class selector, else it is
//! a type selector. Two selectors of one tier do not count components against
//! each other; the later rule wins.
//!
//! # Matching
//!
//! Right to left, as browsers and RmlUi match: the rightmost compound against
//! the node, then each combinator walks up. Every pseudo-class matches against
//! the [`PseudoClasses`] the tree resolved for the node; see
//! [`crate::tree::focus`] for when `:focus`, `:engaged` and `:disabled` are
//! set.

use core::ops::{BitAnd, BitOr};

use cssparser::{ParseError, Parser, Token};

/// A set of pseudo-classes, as bits: a node's state, or what a selector
/// tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PseudoClasses(u8);

impl PseudoClasses {
    /// The empty set.
    pub const NONE: Self = Self(0);
    /// `:hover`: the pointer is over the node or something inside it, while
    /// the pointer is the device driving.
    pub const HOVER: Self = Self(1);
    /// `:active`: the node holds the pointer's press.
    pub const ACTIVE: Self = Self(1 << 1);
    /// `:focus`: the node holds the focus, while the pad or the keyboard is
    /// the device driving.
    pub const FOCUS: Self = Self(1 << 2);
    /// `:disabled`: the node's behavior is disabled.
    pub const DISABLED: Self = Self(1 << 3);
    /// `:engaged`: the focused widget is taking the navigation input.
    pub const ENGAGED: Self = Self(1 << 4);

    /// The pseudo-class `name` spells, ignoring ASCII case.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(cssparser::match_ignore_ascii_case! { name,
            "hover" => Self::HOVER,
            "active" => Self::ACTIVE,
            "focus" => Self::FOCUS,
            "disabled" => Self::DISABLED,
            "engaged" => Self::ENGAGED,
            _ => return None,
        })
    }

    /// Whether every class in `other` is in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for PseudoClasses {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitAnd for PseudoClasses {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

/// A selector's tier in the cascade; see the module docs. Ordered, so a higher
/// tier compares greater.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Specificity {
    /// Types and `*` only.
    Type,
    /// A class or a pseudo-class somewhere in it.
    Class,
    /// An id somewhere in it.
    Id,
}

/// One compound selector: everything between two combinators.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Compound {
    /// Lowercased; `None` for `*` or no type at all.
    pub type_name: Option<Box<str>>,
    pub id: Option<Box<str>>,
    pub classes: Vec<Box<str>>,
    pub pseudo: PseudoClasses,
}

/// How a compound relates to the one on its right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Combinator {
    /// Whitespace: any ancestor.
    Descendant,
    /// `>`: the parent.
    Child,
}

/// A parsed selector, stored right to left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    /// The rightmost compound, which the node itself must match.
    pub(crate) subject: Compound,
    /// Every compound to its left, nearest first, each with the combinator
    /// that joins it to the compound on its right.
    pub(crate) ancestors: Vec<(Combinator, Compound)>,
}

/// What the rule index files a selector under: its subject's id, else its
/// first class, else its type, else nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bucket<'a> {
    Id(&'a str),
    Class(&'a str),
    Type(&'a str),
    Universal,
}

/// The view of a node that matching reads. The tree implements it over its
/// frame; the tests implement it over a table.
pub(crate) trait Element: Sized {
    /// `block`, `span` or a widget name, as the node declared it.
    fn type_name(&self) -> &str;
    fn id(&self) -> Option<&str>;
    fn has_class(&self, class: &str) -> bool;
    fn pseudo(&self) -> PseudoClasses;
    fn parent(&self) -> Option<Self>;
}

impl Compound {
    fn matches(&self, element: &impl Element) -> bool {
        self.type_name
            .as_deref()
            .is_none_or(|name| element.type_name().eq_ignore_ascii_case(name))
            && self.id.as_deref().is_none_or(|id| element.id() == Some(id))
            && self.classes.iter().all(|class| element.has_class(class))
            && element.pseudo().contains(self.pseudo)
    }

    const fn is_empty(&self) -> bool {
        self.type_name.is_none()
            && self.id.is_none()
            && self.classes.is_empty()
            && self.pseudo.is_empty()
    }
}

impl Selector {
    /// Parses one selector list — the part of a rule before its `{` — from
    /// `css`, for a caller outside a stylesheet.
    ///
    /// # Errors
    ///
    /// A message naming what was not understood.
    pub fn parse_list(css: &str) -> Result<Vec<Self>, String> {
        let mut parser = Parser::new(css);
        parse_selector_list(&mut parser).map_err(|error| match error.kind {
            cssparser::ParseErrorKind::Custom(message) => message,
            cssparser::ParseErrorKind::Basic(kind) => kind.to_string(),
        })
    }

    /// The tier this selector cascades in.
    #[must_use]
    pub fn specificity(&self) -> Specificity {
        let compounds =
            || core::iter::once(&self.subject).chain(self.ancestors.iter().map(|(_, c)| c));
        if compounds().any(|compound| compound.id.is_some()) {
            Specificity::Id
        } else if compounds()
            .any(|compound| !compound.classes.is_empty() || !compound.pseudo.is_empty())
        {
            Specificity::Class
        } else {
            Specificity::Type
        }
    }

    /// The pseudo-classes the node itself is tested for.
    pub(crate) fn subject_pseudo(&self) -> PseudoClasses {
        self.subject.pseudo
    }

    /// The pseudo-classes some ancestor is tested for.
    pub(crate) fn ancestor_pseudo(&self) -> PseudoClasses {
        self.ancestors
            .iter()
            .fold(PseudoClasses::NONE, |all, (_, compound)| {
                all | compound.pseudo
            })
    }

    /// Whether matching this selector reads anything above the node.
    pub(crate) fn has_ancestors(&self) -> bool {
        !self.ancestors.is_empty()
    }

    pub(crate) fn bucket(&self) -> Bucket<'_> {
        let subject = &self.subject;
        if let Some(id) = subject.id.as_deref() {
            Bucket::Id(id)
        } else if let Some(class) = subject.classes.first() {
            Bucket::Class(class)
        } else if let Some(name) = subject.type_name.as_deref() {
            Bucket::Type(name)
        } else {
            Bucket::Universal
        }
    }

    /// Whether `element` matches, right to left.
    pub(crate) fn matches(&self, element: &impl Element) -> bool {
        self.subject.matches(element) && self.ancestors_match(0, element)
    }

    /// Whether `ancestors[from..]` match above `element`, trying every
    /// ancestor a descendant combinator could mean before giving up.
    fn ancestors_match(&self, from: usize, element: &impl Element) -> bool {
        let Some((combinator, compound)) = self.ancestors.get(from) else {
            return true;
        };
        match combinator {
            Combinator::Child => element.parent().is_some_and(|parent| {
                compound.matches(&parent) && self.ancestors_match(from + 1, &parent)
            }),
            Combinator::Descendant => {
                let mut next = element.parent();
                while let Some(ancestor) = next {
                    if compound.matches(&ancestor) && self.ancestors_match(from + 1, &ancestor) {
                        return true;
                    }
                    next = ancestor.parent();
                }
                false
            }
        }
    }
}

/// The error a selector refuses with: a message for the diagnostic.
pub(crate) type SelectorError = ParseError<String>;

fn refuse<T>(message: impl Into<String>) -> Result<T, SelectorError> {
    Err(ParseError::custom(message.into()))
}

/// A comma-separated list of selectors, which must use the whole of `input`.
pub(crate) fn parse_selector_list<'i>(
    input: &mut Parser<'i>,
) -> Result<Vec<Selector>, SelectorError> {
    input.parse_comma_separated(parse_selector)
}

fn parse_selector<'i>(input: &mut Parser<'i>) -> Result<Selector, SelectorError> {
    // Left to right as written, reversed at the end.
    let mut compounds = vec![parse_compound(input)?];
    let mut combinators = Vec::new();
    loop {
        let mut spaced = false;
        loop {
            let before = input.state();
            match input.next_including_whitespace() {
                Ok(Token::WhiteSpace(_)) => spaced = true,
                Ok(_) => {
                    input.reset(&before);
                    break;
                }
                Err(_) => break,
            }
        }
        let before = input.state();
        let combinator = match input.next_including_whitespace() {
            Err(_) => break,
            Ok(Token::Delim('>')) => {
                input.skip_whitespace();
                Combinator::Child
            }
            Ok(Token::Delim(delim @ ('+' | '~'))) => {
                return refuse(format!("the `{delim}` sibling combinator is not supported"));
            }
            Ok(_) if spaced => {
                input.reset(&before);
                Combinator::Descendant
            }
            Ok(token) => {
                let token = token.clone();
                return refuse(format!("unexpected {token:?} in a selector"));
            }
        };
        combinators.push(combinator);
        compounds.push(parse_compound(input)?);
    }

    let subject = compounds
        .pop()
        .expect("a selector has at least one compound");
    let ancestors = combinators
        .into_iter()
        .rev()
        .zip(compounds.into_iter().rev())
        .collect();
    Ok(Selector { subject, ancestors })
}

fn parse_compound<'i>(input: &mut Parser<'i>) -> Result<Compound, SelectorError> {
    let mut compound = Compound::default();
    let mut universal = false;
    let mut first = true;
    loop {
        let before = input.state();
        let Ok(token) = input.next_including_whitespace() else {
            break;
        };
        match token.clone() {
            Token::Ident(name) if first => {
                compound.type_name = Some(name.to_ascii_lowercase().into_boxed_str());
            }
            Token::Delim('*') if first => universal = true,
            Token::IDHash(id) => {
                if compound.id.is_some() {
                    return refuse(format!(
                        "a compound selector names two ids, the second `#{id}`"
                    ));
                }
                compound.id = Some(Box::from(&*id));
            }
            Token::Hash(hash) => return refuse(format!("`#{hash}` is not a valid id")),
            Token::Delim('.') => match input.next_including_whitespace() {
                Ok(Token::Ident(class)) => compound.classes.push(Box::from(&**class)),
                _ => return refuse("`.` is not followed by a class name"),
            },
            Token::Colon => match input.next_including_whitespace() {
                Ok(Token::Ident(name)) => match PseudoClasses::from_name(name) {
                    Some(pseudo) => compound.pseudo = compound.pseudo | pseudo,
                    None => return refuse(format!("the pseudo-class `:{name}` is not supported")),
                },
                Ok(Token::Colon) => return refuse("pseudo-elements are not supported"),
                Ok(Token::Function(name)) => {
                    return refuse(format!("the pseudo-class `:{name}()` is not supported"));
                }
                _ => return refuse("`:` is not followed by a pseudo-class"),
            },
            Token::SquareBracketBlock => return refuse("attribute selectors are not supported"),
            _ => {
                input.reset(&before);
                break;
            }
        }
        first = false;
    }
    if compound.is_empty() && !universal {
        let found = input.next().cloned();
        return match found {
            Ok(token) => refuse(format!("expected a selector, found {token:?}")),
            Err(_) => refuse("expected a selector"),
        };
    }
    Ok(compound)
}

/// A node's own selector as a builder took it — `type#id.class.class`, every
/// part optional — split into its parts without allocating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NodeSelector<'a> {
    pub type_name: Option<&'a str>,
    pub id: Option<&'a str>,
    /// Everything after the type: each id and class with its marker.
    tail: &'a str,
    text: &'a str,
}

impl<'a> NodeSelector<'a> {
    /// No type, id or class.
    pub const EMPTY: Self = Self {
        type_name: None,
        id: None,
        tail: "",
        text: "",
    };

    /// The selector as written.
    pub const fn text(self) -> &'a str {
        self.text
    }

    /// Splits `text`, or refuses it when a part is empty, two ids are named, or
    /// a name holds a character outside `[A-Za-z0-9_-]` and non-ASCII.
    pub fn parse(text: &'a str) -> Result<Self, ()> {
        let is_name = |name: &str| {
            !name.is_empty()
                && name.chars().all(|ch| {
                    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || !ch.is_ascii()
                })
        };
        let (head, tail) = text.split_at(text.find(['#', '.']).unwrap_or(text.len()));
        let type_name = match head {
            "" => None,
            head if is_name(head) => Some(head),
            _ => return Err(()),
        };
        let mut id = None;
        for (marker, name) in segments(tail) {
            if !is_name(name) || marker == b'#' && id.replace(name).is_some() {
                return Err(());
            }
        }
        Ok(Self {
            type_name,
            id,
            tail,
            text,
        })
    }

    /// Every class the node declares.
    pub fn classes(self) -> impl Iterator<Item = &'a str> {
        segments(self.tail).filter_map(|(marker, name)| (marker == b'.').then_some(name))
    }
}

/// `#a.b.c` as `(b'#', "a")`, `(b'.', "b")`, `(b'.', "c")`.
fn segments(tail: &str) -> impl Iterator<Item = (u8, &str)> {
    let mut rest = tail;
    core::iter::from_fn(move || {
        let marker = *rest.as_bytes().first()?;
        let body = &rest[1..];
        let end = body.find(['#', '.']).unwrap_or(body.len());
        rest = &body[end..];
        Some((marker, &body[..end]))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node in a table: its selector text and its parent's row.
    struct Row {
        kind: &'static str,
        selector: &'static str,
        pseudo: PseudoClasses,
        parent: Option<usize>,
    }

    #[derive(Clone, Copy)]
    struct TableElement<'a> {
        rows: &'a [Row],
        at: usize,
    }

    impl Element for TableElement<'_> {
        fn type_name(&self) -> &str {
            let row = &self.rows[self.at];
            NodeSelector::parse(row.selector)
                .expect("valid")
                .type_name
                .unwrap_or(row.kind)
        }
        fn id(&self) -> Option<&str> {
            NodeSelector::parse(self.rows[self.at].selector)
                .expect("valid")
                .id
        }
        fn has_class(&self, class: &str) -> bool {
            NodeSelector::parse(self.rows[self.at].selector)
                .expect("valid")
                .classes()
                .any(|own| own == class)
        }
        fn pseudo(&self) -> PseudoClasses {
            self.rows[self.at].pseudo
        }
        fn parent(&self) -> Option<Self> {
            self.rows[self.at].parent.map(|at| Self {
                rows: self.rows,
                at,
            })
        }
    }

    fn one(css: &str) -> Selector {
        let mut list = Selector::parse_list(css).unwrap_or_else(|why| panic!("{css}: {why}"));
        assert_eq!(list.len(), 1, "{css}");
        list.remove(0)
    }

    /// `panel > row.selected:hover`'s shape, with a `#stats` block above.
    const TREE: &[Row] = &[
        Row {
            kind: "block",
            selector: "#stats.panel",
            pseudo: PseudoClasses::NONE,
            parent: None,
        },
        Row {
            kind: "block",
            selector: "list.rows",
            pseudo: PseudoClasses::HOVER,
            parent: Some(0),
        },
        Row {
            kind: "block",
            selector: ".row.selected",
            pseudo: PseudoClasses::HOVER,
            parent: Some(1),
        },
        Row {
            kind: "span",
            selector: ".label",
            pseudo: PseudoClasses::NONE,
            parent: Some(2),
        },
        Row {
            kind: "span",
            selector: "#value",
            pseudo: PseudoClasses::ACTIVE,
            parent: Some(2),
        },
    ];

    /// **Every selector form matches exactly the nodes it names**, across
    /// types, ids, classes, pseudo-classes and both combinators — a table of
    /// selectors against the five nodes above, each row the set it must match.
    #[test]
    fn each_selector_matches_exactly_the_nodes_it_names() {
        let cases: &[(&str, &[usize])] = &[
            // A declared type replaces `block`: `list.rows` is not a block.
            ("block", &[0, 2]),
            ("span", &[3, 4]),
            ("LIST", &[1]),
            ("*", &[0, 1, 2, 3, 4]),
            ("#stats", &[0]),
            ("block#stats.panel", &[0]),
            ("span#stats", &[]),
            (".row", &[2]),
            (".row.selected", &[2]),
            (".row.missing", &[]),
            (":hover", &[1, 2]),
            (".row:hover", &[2]),
            (".label:hover", &[]),
            ("span:active", &[4]),
            (":focus", &[]),
            (":disabled", &[]),
            (":engaged", &[]),
            ("#stats span", &[3, 4]),
            ("#stats > span", &[]),
            (".row > span", &[3, 4]),
            ("list > .row > #value", &[4]),
            (".panel .rows .selected .label", &[3]),
            // The descendant combinator must try every ancestor, not just the
            // nearest one that matches the compound.
            (".panel block span", &[3, 4]),
            // `*` first meets `.row`, whose parent is not `.panel`; only the
            // list above it is.
            (".panel > * span", &[3, 4]),
            (".panel > * > * > span", &[3, 4]),
            (".panel > block span", &[]),
            ("list:hover .label", &[3]),
            (".panel:hover span", &[]),
        ];
        for (css, want) in cases {
            let selector = one(css);
            let got: Vec<usize> = (0..TREE.len())
                .filter(|&at| selector.matches(&TableElement { rows: TREE, at }))
                .collect();
            assert_eq!(&got, want, "`{css}`");
        }
    }

    /// **Specificity is the tier of the strongest part**: an id anywhere beats
    /// any count of classes, and a pseudo-class is a class.
    #[test]
    fn specificity_is_the_tier_of_the_strongest_component() {
        let cases = [
            ("block", Specificity::Type),
            ("block span", Specificity::Type),
            ("*", Specificity::Type),
            (".a", Specificity::Class),
            (".a.b.c.d", Specificity::Class),
            (":hover", Specificity::Class),
            ("block > span:active", Specificity::Class),
            ("#x", Specificity::Id),
            ("#x span", Specificity::Id),
            ("block .a #x", Specificity::Id),
        ];
        for (css, want) in cases {
            assert_eq!(one(css).specificity(), want, "`{css}`");
        }
        assert!(Specificity::Id > Specificity::Class && Specificity::Class > Specificity::Type);
    }

    /// **A list is one selector per comma, whitespace and comments around
    /// the parts are ignored, and an escape is decoded** by the tokenizer.
    #[test]
    fn lists_whitespace_comments_and_escapes_parse() {
        let list = Selector::parse_list(" .a ,/* note */#b>span , c  d ").expect("parses");
        assert_eq!(list.len(), 3);
        assert_eq!(list[1].subject.type_name.as_deref(), Some("span"));
        assert_eq!(list[1].ancestors[0].0, Combinator::Child);
        assert_eq!(list[2].ancestors[0].0, Combinator::Descendant);

        let escaped = one(r".a\:b");
        assert_eq!(escaped.subject.classes, [Box::from("a:b")]);
        let escaped = one(r"#\31 23");
        assert_eq!(escaped.subject.id.as_deref(), Some("123"));
    }

    /// **Everything outside the grammar is refused with a message saying
    /// what**, never parsed into a selector that means something else.
    #[test]
    fn everything_outside_the_grammar_is_refused_by_name() {
        let cases = [
            ("a + b", "sibling"),
            ("a ~ b", "sibling"),
            (":nth-child(2)", "not supported"),
            (":visited", "`:visited`"),
            ("::before", "pseudo-elements"),
            ("[href]", "attribute"),
            ("#1a", "not a valid id"),
            ("#a#b", "two ids"),
            (". a", "class name"),
            ("a,", "expected a selector"),
            ("a >", "expected a selector"),
            ("> a", "expected a selector"),
            ("", "expected a selector"),
        ];
        for (css, message) in cases {
            let error = Selector::parse_list(css).expect_err(css);
            assert!(
                error.contains(message),
                "`{css}` refused with {error:?}, not {message:?}"
            );
        }
    }

    /// **A rule depends on the pseudo-classes it names, split by where**:
    /// what the node is tested for, and what an ancestor is.
    #[test]
    fn pseudo_dependencies_are_split_between_the_node_and_its_ancestors() {
        let selector = one(".list:hover > .row:active:focus");
        assert_eq!(
            selector.subject_pseudo(),
            PseudoClasses::ACTIVE | PseudoClasses::FOCUS
        );
        assert_eq!(selector.ancestor_pseudo(), PseudoClasses::HOVER);
        assert!(selector.has_ancestors());
        assert!(!one(".row").has_ancestors());
        assert!(one(".row").subject_pseudo().is_empty());
    }

    /// The bucket is the subject's id, else its first class, else its type.
    #[test]
    fn a_selector_is_bucketed_by_its_subjects_strongest_part() {
        assert_eq!(one("block .a #x.b").bucket(), Bucket::Id("x"));
        assert_eq!(one("#x .b.c").bucket(), Bucket::Class("b"));
        assert_eq!(one(".a SPAN").bucket(), Bucket::Type("span"));
        assert_eq!(one(".a :hover").bucket(), Bucket::Universal);
    }

    /// **A builder's selector splits into type, id and classes**, and a
    /// malformed one is refused rather than guessed at.
    #[test]
    fn a_node_selector_splits_into_its_parts() {
        let parsed = NodeSelector::parse("button#ok.primary.wide").expect("valid");
        assert_eq!(parsed.type_name, Some("button"));
        assert_eq!(parsed.id, Some("ok"));
        assert_eq!(parsed.classes().collect::<Vec<_>>(), ["primary", "wide"]);

        let parsed = NodeSelector::parse(".a#id.b").expect("valid");
        assert_eq!((parsed.type_name, parsed.id), (None, Some("id")));
        assert_eq!(parsed.classes().collect::<Vec<_>>(), ["a", "b"]);

        let empty = NodeSelector::parse("").expect("valid");
        assert_eq!(
            (empty.type_name, empty.id, empty.classes().count()),
            (None, None, 0)
        );

        for bad in [
            "#", ".", "a b", "#a#b", ".a#b#c", "a:hover", "..a", "#a.", "a>b",
        ] {
            assert!(NodeSelector::parse(bad).is_err(), "`{bad}` was accepted");
        }
    }
}
