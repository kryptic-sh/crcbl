//! What the derive reads off a type: each variant's message and the role each
//! of its fields plays.
//!
//! Everything here refuses rather than ignores. An attribute this derive does
//! not implement, in a place it does not belong, or in a combination that does
//! not mean anything is an error spanned at that attribute: the alternative is
//! an error type that compiles and then says or chains something other than
//! what its author wrote.

use proc_macro2::{Span, TokenStream};
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Fields, Ident, LitStr, Member, Token, Type};

/// What one `#[error(…)]` says. No `Debug`, for the reason
/// `crcbl-reflect-derive` gives for its own: `syn`'s types implement it only
/// behind its `extra-traits` feature.
pub(crate) enum Message {
    /// `#[error(transparent)]`: `Display` and `source` are the one field's own.
    /// The span is the keyword's, for the refusals that point at it.
    Transparent(Span),
    /// `#[error("…", args…)]`: the format string, and whatever followed its
    /// comma, still unrewritten.
    Format { fmt: LitStr, args: TokenStream },
}

/// One field of a variant (or of the struct).
pub(crate) struct Field<'a> {
    /// How the field is named in a pattern: its ident, or its index.
    pub(crate) member: Member,
    /// The local the expansion's patterns bind it to: the field's own name for
    /// a named field, `_0`, `_1`, … for a tuple one. The `.0` shorthand and a
    /// `{0}` in a message are rewritten to the same `_0`.
    pub(crate) binding: Ident,
    /// The field's type, whose span is where a field that is not an error is
    /// reported.
    pub(crate) ty: &'a Type,
    /// `#[from]` on the field.
    pub(crate) from: Option<Span>,
    /// Whether `source()` returns this field: `#[source]`, `#[from]`, or a
    /// named field called `source` when no field carries either attribute.
    pub(crate) is_source: bool,
}

/// How a variant's fields are written, which is what its pattern and its
/// constructor have to match.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    Named,
    Unnamed,
    Unit,
}

/// One variant, or the struct itself.
pub(crate) struct Variant<'a> {
    /// The variant's name, or `None` for a struct.
    pub(crate) ident: Option<&'a Ident>,
    pub(crate) style: Style,
    pub(crate) fields: Vec<Field<'a>>,
    pub(crate) message: Message,
}

/// The type the derive is on: a struct is one variant with no name.
pub(crate) struct Shape<'a> {
    pub(crate) variants: Vec<Variant<'a>>,
}

/// Reads and checks every attribute the derive owns.
///
/// # Errors
///
/// A union; an enum-level `#[error]`; a variant or struct with no `#[error]`,
/// or two; `#[error]` with anything other than a string literal (and optional
/// format arguments) or `transparent`, notably thiserror's `fmt = …`; `#[from]`
/// or `#[source]` anywhere but on a field, with arguments, or twice; two source
/// fields; `#[from]` beside another field; `transparent` without exactly one
/// field, or with `#[source]` on it; a `Backtrace` field; an `Option` source.
pub(crate) fn read(input: &DeriveInput) -> syn::Result<Shape<'_>> {
    not_on_container(&input.attrs, "from")?;
    not_on_container(&input.attrs, "source")?;

    match &input.data {
        Data::Struct(data) => {
            let message = message(&input.attrs, input.ident.span(), "the struct")?;
            Ok(Shape {
                variants: vec![variant(None, &data.fields, message)?],
            })
        }
        Data::Enum(data) => {
            if let Some(attr) = find(&input.attrs, "error") {
                return Err(syn::Error::new(
                    attr.span(),
                    "an `#[error]` on the enum itself is not supported; put one on each variant",
                ));
            }
            let variants = data
                .variants
                .iter()
                .map(|v| {
                    not_on_container(&v.attrs, "from")?;
                    not_on_container(&v.attrs, "source")?;
                    let message = message(&v.attrs, v.ident.span(), "this variant")?;
                    variant(Some(&v.ident), &v.fields, message)
                })
                .collect::<syn::Result<_>>()?;
            Ok(Shape { variants })
        }
        Data::Union(data) => Err(syn::Error::new(
            data.union_token.span(),
            "`#[derive(Error)]` does not implement a union: which field is live is not \
             knowable from the value",
        )),
    }
}

fn find<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a Attribute> {
    attrs.iter().find(|attr| attr.path().is_ident(name))
}

/// `#[from]` and `#[source]` name a field, so anywhere else they are a mistake.
fn not_on_container(attrs: &[Attribute], name: &str) -> syn::Result<()> {
    match find(attrs, name) {
        Some(attr) => Err(syn::Error::new(
            attr.span(),
            format!("`#[{name}]` belongs on a field, not here"),
        )),
        None => Ok(()),
    }
}

/// The one `#[error(…)]` a struct or variant must carry.
fn message(attrs: &[Attribute], owner: Span, what: &str) -> syn::Result<Message> {
    let mut found = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("error")) {
        if found.is_some() {
            return Err(syn::Error::new(
                attr.span(),
                "`#[error]` is set twice; one message per variant",
            ));
        }
        found = Some(attr.parse_args_with(parse_message)?);
    }
    found.ok_or_else(|| {
        syn::Error::new(
            owner,
            format!("missing `#[error(\"…\")]` or `#[error(transparent)]` on {what}"),
        )
    })
}

fn parse_message(input: ParseStream<'_>) -> syn::Result<Message> {
    if input.peek(LitStr) {
        let fmt: LitStr = input.parse()?;
        let args = if input.is_empty() {
            TokenStream::new()
        } else {
            input.parse::<Token![,]>()?;
            input.parse()?
        };
        return Ok(Message::Format { fmt, args });
    }
    if input.peek(Ident) {
        let ident: Ident = input.parse()?;
        if ident == "transparent" && input.is_empty() {
            return Ok(Message::Transparent(ident.span()));
        }
        if ident == "fmt" {
            return Err(syn::Error::new(
                ident.span(),
                "`#[error(fmt = …)]` is not supported; write the message as a format string",
            ));
        }
    }
    Err(input.error("expected a format string, `#[error(\"…\")]`, or `#[error(transparent)]`"))
}

fn variant<'a>(
    ident: Option<&'a Ident>,
    fields: &'a Fields,
    message: Message,
) -> syn::Result<Variant<'a>> {
    let style = match fields {
        Fields::Named(_) => Style::Named,
        Fields::Unnamed(_) => Style::Unnamed,
        Fields::Unit => Style::Unit,
    };

    let mut read = Vec::new();
    let mut source_attr: Option<Span> = None;
    for (index, field) in fields.iter().enumerate() {
        if let Some(attr) = find(&field.attrs, "error") {
            return Err(syn::Error::new(
                attr.span(),
                "`#[error]` belongs on the struct or on a variant, not on a field",
            ));
        }
        if is_named(&field.ty, "Backtrace") {
            return Err(syn::Error::new(
                field.ty.span(),
                "a `Backtrace` field is not supported: this derive does not provide one",
            ));
        }
        let from = marker(&field.attrs, "from")?;
        let source = marker(&field.attrs, "source")?;
        let explicit = from.or(source);
        if let Some(span) = explicit {
            if source_attr.is_some() {
                return Err(syn::Error::new(
                    span,
                    "only one field can be the source: `#[source]`, or `#[from]`, which implies it",
                ));
            }
            source_attr = Some(span);
        }
        let (member, binding) = match &field.ident {
            Some(name) => (Member::Named(name.clone()), name.clone()),
            None => (
                Member::Unnamed(syn::Index::from(index)),
                Ident::new(&format!("_{index}"), field.span()),
            ),
        };
        read.push(Field {
            member,
            binding,
            ty: &field.ty,
            from,
            is_source: explicit.is_some(),
        });
    }

    // thiserror's rule, which `crcbl-scene`'s `ScnError` and others rely on: a
    // named field called `source` is the source when no field says otherwise.
    if source_attr.is_none()
        && let Some(field) = read
            .iter_mut()
            .find(|f| matches!(&f.member, Member::Named(name) if name == "source"))
    {
        field.is_source = true;
    }

    if let Some(field) = read.iter().find(|f| f.is_source)
        && is_named(field.ty, "Option")
    {
        return Err(syn::Error::new(
            field.ty.span(),
            "an optional source is not supported; the source field must be an error",
        ));
    }

    if let Some(span) = read.iter().find_map(|f| f.from)
        && read.len() != 1
    {
        return Err(syn::Error::new(
            span,
            "`#[from]` must be on the only field: `From` has nothing to fill the others with",
        ));
    }

    if let Message::Transparent(span) = &message {
        if read.len() != 1 {
            return Err(syn::Error::new(
                *span,
                "`#[error(transparent)]` needs exactly one field to forward to",
            ));
        }
        if let Some(field) = fields.iter().next()
            && let Some(attr) = find(&field.attrs, "source")
        {
            return Err(syn::Error::new(
                attr.span(),
                "`#[source]` is not expected on a transparent field: its own `source()` is \
                 forwarded already",
            ));
        }
    }

    Ok(Variant {
        ident,
        style,
        fields: read,
        message,
    })
}

/// A bare `#[from]` or `#[source]`, at most once.
fn marker(attrs: &[Attribute], name: &str) -> syn::Result<Option<Span>> {
    let mut found = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident(name)) {
        if !matches!(attr.meta, syn::Meta::Path(_)) {
            return Err(syn::Error::new(
                attr.span(),
                format!("`#[{name}]` takes no arguments"),
            ));
        }
        if found.is_some() {
            return Err(syn::Error::new(
                attr.span(),
                format!("`#[{name}]` is set twice"),
            ));
        }
        found = Some(attr.span());
    }
    Ok(found)
}

/// Whether `ty`, through any references and parentheses, is a path whose last
/// segment is `name`. A syntactic test, so an alias is not seen through.
pub(crate) fn is_named(ty: &Type, name: &str) -> bool {
    match ty {
        Type::Reference(reference) => is_named(&reference.elem, name),
        Type::Paren(paren) => is_named(&paren.elem, name),
        Type::Group(group) => is_named(&group.elem, name),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == name),
        _ => false,
    }
}
