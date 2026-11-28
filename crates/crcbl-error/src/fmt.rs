//! Turning an `#[error("…", args…)]` into the arguments of a `format_args!`
//! that runs with the variant's fields bound as locals.
//!
//! A named field is bound under its own name, so `{field}` is an implicit
//! capture and needs no rewriting. Three things do, each as thiserror does it,
//! so a message reads the same after a crate moves over:
//!
//! * `{0}` names a tuple field, not the first extra argument, so it becomes
//!   `{_0}`, the local that field is bound to.
//! * `.field` and `.0` at the start of an extra argument are the field, so the
//!   dot goes (`.field` → `field`) or the index becomes its local (`.0` →
//!   `_0`, `.0.1` → `_0.1`).
//! * A bare `{path}` of a `Path` or `PathBuf` field displays it with
//!   `Path::display`, since neither implements `Display` itself.
//!
//! The arguments are rewritten as tokens rather than parsed as expressions
//! because the workspace's `syn` is built without `full`, which is the
//! expression grammar.

use std::collections::BTreeSet;

use proc_macro2::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use quote::quote;
use syn::ext::IdentExt;
use syn::{LitStr, Member};

use crate::attrs::{Field, is_named};

/// The tokens inside `format_args!(…)`: the rewritten string, the rewritten
/// arguments, and a named argument per `Path` field the string displays.
///
/// # Errors
///
/// A `{0}` in a variant with tuple fields that also has an unnamed extra
/// argument, which thiserror refuses as ambiguous for the same reason.
pub(crate) fn arguments(
    fmt: &LitStr,
    args: &TokenStream,
    fields: &[Field<'_>],
) -> syn::Result<TokenStream> {
    let explicit = scan(args);
    let positional_allowed = fields
        .iter()
        .all(|field| matches!(field.member, Member::Named(_)));

    let value = fmt.value();
    let mut read = value.as_str();
    let mut out = String::new();
    let mut displayed: Vec<(String, &Field<'_>)> = Vec::new();

    while let Some(brace) = read.find('{') {
        out.push_str(&read[..=brace]);
        read = &read[brace + 1..];
        if let Some(rest) = read.strip_prefix('{') {
            out.push('{');
            read = rest;
            continue;
        }
        let Some(next) = read.chars().next() else {
            break;
        };
        let field = match next {
            '0'..='9' => {
                let digits = take(&mut read, |c| c.is_ascii_digit());
                if !positional_allowed && let Some(span) = explicit.first_unnamed {
                    return Err(syn::Error::new(
                        span,
                        "ambiguous reference to positional arguments by number in a tuple \
                         variant; change this to a named argument",
                    ));
                }
                let found = digits.parse::<u32>().ok().and_then(|index| {
                    fields.iter().find(
                        |field| matches!(&field.member, Member::Unnamed(i) if i.index == index),
                    )
                });
                match found {
                    Some(field) => field,
                    None => {
                        out.push_str(digits);
                        continue;
                    }
                }
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                if read.starts_with("r#") {
                    continue;
                }
                let name = take(&mut read, |c| c.is_ascii_alphanumeric() || c == '_');
                let found = (!explicit.named.contains(name))
                    .then(|| {
                        fields.iter().find(|field| {
                            matches!(&field.member, Member::Named(ident) if ident.unraw() == name)
                        })
                    })
                    .flatten();
                match found {
                    Some(field) => field,
                    None => {
                        out.push_str(name);
                        continue;
                    }
                }
            }
            _ => continue,
        };

        // No format spec at all — not even an empty `:` — is thiserror's cue to
        // display a path through `Path::display`.
        if read.starts_with('}') && is_path(field) {
            let mut var = match &field.member {
                Member::Named(ident) => format!("__display_{}", ident.unraw()),
                Member::Unnamed(index) => format!("__display{}", index.index),
            };
            while explicit.named.contains(&var) {
                var.insert(0, '_');
            }
            out.push_str(&var);
            if !displayed.iter().any(|(seen, _)| *seen == var) {
                displayed.push((var, field));
            }
        } else {
            out.push_str(&field.binding.unraw().to_string());
        }
    }
    out.push_str(read);

    let fmt = LitStr::new(&out, fmt.span());
    let mut tokens = quote!(#fmt);
    let args = shorthand(trim_trailing_comma(args.clone()), true);
    if !args.is_empty() {
        tokens.extend(quote!(, #args));
    }
    for (var, field) in displayed {
        let var = Ident::new(&var, fmt.span());
        let binding = &field.binding;
        tokens.extend(quote!(, #var = #binding.display()));
    }
    Ok(tokens)
}

fn is_path(field: &Field<'_>) -> bool {
    is_named(field.ty, "Path") || is_named(field.ty, "PathBuf")
}

/// Splits off the leading run of `read` that `keep` accepts.
fn take<'a>(read: &mut &'a str, keep: impl Fn(char) -> bool) -> &'a str {
    let end = read.find(|c| !keep(c)).unwrap_or(read.len());
    let (taken, rest) = read.split_at(end);
    *read = rest;
    taken
}

/// What the user's own extra arguments are: the names given with `name = …`,
/// which shadow a field of the same name in the string, and where the first
/// unnamed one is, for the ambiguity refusal.
struct Explicit {
    named: BTreeSet<String>,
    first_unnamed: Option<Span>,
}

fn scan(args: &TokenStream) -> Explicit {
    let mut explicit = Explicit {
        named: BTreeSet::new(),
        first_unnamed: None,
    };
    let mut segment: Vec<TokenTree> = Vec::new();
    let mut finish = |segment: &mut Vec<TokenTree>| {
        match segment.as_slice() {
            [] => {}
            [TokenTree::Ident(name), TokenTree::Punct(eq), ..]
                if eq.as_char() == '=' && eq.spacing() == Spacing::Alone =>
            {
                explicit.named.insert(name.unraw().to_string());
            }
            [first, ..] => {
                explicit.first_unnamed.get_or_insert(first.span());
            }
        }
        segment.clear();
    };
    for token in args.clone() {
        match &token {
            TokenTree::Punct(punct) if punct.as_char() == ',' => finish(&mut segment),
            _ => segment.push(token),
        }
    }
    finish(&mut segment);
    explicit
}

fn trim_trailing_comma(args: TokenStream) -> TokenStream {
    let mut tokens: Vec<TokenTree> = args.into_iter().collect();
    if matches!(tokens.last(), Some(TokenTree::Punct(p)) if p.as_char() == ',') {
        tokens.pop();
    }
    tokens.into_iter().collect()
}

/// Rewrites thiserror's `.field` / `.0` shorthand wherever an expression can
/// begin: the start of an argument, after an operator or one of the keywords
/// that take an expression, and the start of a bracketed group. The set is
/// thiserror's own (`thiserror-impl`'s `parse_token_expr`), less its
/// turbofish tracking: that only decides a `.` directly after the `>` closing
/// a `::<…>`, which no valid argument contains.
fn shorthand(tokens: TokenStream, mut begin: bool) -> TokenStream {
    let mut out: Vec<TokenTree> = Vec::new();
    let mut iter = tokens.into_iter().peekable();
    while let Some(token) = iter.next() {
        if begin
            && let TokenTree::Punct(dot) = &token
            && dot.as_char() == '.'
        {
            match iter.peek() {
                Some(TokenTree::Ident(_)) => {
                    // `.field` → `field`: drop the dot, keep the name.
                    begin = false;
                    continue;
                }
                Some(TokenTree::Literal(literal)) => {
                    if let Some(replaced) = index_literal(literal) {
                        iter.next();
                        out.extend(replaced);
                        begin = false;
                        continue;
                    }
                }
                _ => {}
            }
        }

        let next_begins = match &token {
            TokenTree::Punct(punct) => matches!(
                punct.as_char(),
                '+' | '&' | '!' | '^' | ',' | '/' | '=' | '>' | '<' | '|' | '%' | ';' | '*' | '-'
            ),
            TokenTree::Ident(ident) => matches!(
                ident.to_string().as_str(),
                "break" | "continue" | "if" | "in" | "match" | "mut" | "return" | "while"
            ),
            TokenTree::Group(_) | TokenTree::Literal(_) => false,
        };

        match token {
            TokenTree::Group(group) if group.delimiter() != Delimiter::None => {
                let mut rewritten = Group::new(group.delimiter(), shorthand(group.stream(), true));
                rewritten.set_span(group.span());
                out.push(TokenTree::Group(rewritten));
            }
            other => out.push(other),
        }
        begin = next_begins;
    }
    out.into_iter().collect()
}

/// `0` → `_0`, and `0.1` (which the lexer reads as one float) → `_0.1`.
fn index_literal(literal: &Literal) -> Option<Vec<TokenTree>> {
    let repr = literal.to_string();
    let span = literal.span();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut parts = repr.split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(first), None, None) if digits(first) => Some(vec![TokenTree::Ident(Ident::new(
            &format!("_{first}"),
            span,
        ))]),
        (Some(first), Some(second), None) if digits(first) && digits(second) => {
            let mut dot = Punct::new('.', Spacing::Alone);
            dot.set_span(span);
            let mut index = Literal::u32_unsuffixed(second.parse().ok()?);
            index.set_span(span);
            Some(vec![
                TokenTree::Ident(Ident::new(&format!("_{first}"), span)),
                TokenTree::Punct(dot),
                TokenTree::Literal(index),
            ])
        }
        _ => None,
    }
}
