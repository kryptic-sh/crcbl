//! The `#[reflect(…)]` attributes, on the container and on a field.
//!
//! Everything here refuses rather than ignores: an unknown key, a key twice, a
//! half-written range and a skipped field that also carries a label are each an
//! error naming what is wrong. A derive that silently dropped an attribute would
//! be a row that quietly lost its slider bounds.

use proc_macro2::Span;
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::{Attribute, Lit, LitStr, Path, Token};

/// The attribute on the type itself.
///
/// No `Debug`: `syn`'s own types only implement it behind its `extra-traits`
/// feature, and a feature turned on to print a struct nothing prints would be
/// parser code compiled into every build of this crate for nothing.
pub(crate) struct Container {
    /// The path `crcbl-reflect` is reachable at from the deriving crate.
    pub(crate) krate: Path,
}

/// The attributes on one field. No `Debug`, for the reason [`Container`] has
/// none.
#[derive(Default)]
pub(crate) struct FieldAttrs {
    /// `#[reflect(skip)]` — no row, and no path segment.
    pub(crate) skip: bool,
    /// `#[reflect(name = "…")]` — what the row is labelled with.
    pub(crate) label: Option<LitStr>,
    /// `#[reflect(min = …)]`, with its span for the refusals.
    pub(crate) min: Option<(f64, Span)>,
    /// `#[reflect(max = …)]`.
    pub(crate) max: Option<(f64, Span)>,
    /// `#[reflect(step = …)]`.
    pub(crate) step: Option<f64>,
}

/// Reads `#[reflect(crate = "…")]` off the type.
///
/// The default is `::crcbl_reflect`, which is right for a crate that names this
/// one directly. A game names the engine and nothing else — `docs/plan/sample/
/// 00-samples-overview.md`'s rule 1 — so it reaches the trait through the
/// umbrella and writes `#[reflect(crate = "crcbl::reflect")]`, exactly the way
/// `apps/breakout` already writes `#[serde(crate = "crcbl::serde")]`.
pub(crate) fn container(attrs: &[Attribute]) -> syn::Result<Container> {
    let mut krate: Option<Path> = None;

    for attr in attrs.iter().filter(|attr| attr.path().is_ident("reflect")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("crate") {
                if krate.is_some() {
                    return Err(meta.error("`crate` is set twice"));
                }
                let lit: LitStr = meta.value()?.parse()?;
                krate = Some(lit.parse()?);
                return Ok(());
            }
            Err(meta
                .error("unknown `reflect` attribute on a type; the only one is `crate = \"…\"`"))
        })?;
    }

    Ok(Container {
        krate: krate.unwrap_or_else(|| syn::parse_quote!(::crcbl_reflect)),
    })
}

/// Reads `#[reflect(…)]` off one field.
///
/// # Errors
///
/// An unknown key, a key set twice, `min` without `max` or the other way round,
/// a `min` above its `max`, or `skip` beside anything else.
pub(crate) fn field(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut parsed = FieldAttrs::default();
    let mut skip_span: Option<Span> = None;

    for attr in attrs.iter().filter(|attr| attr.path().is_ident("reflect")) {
        attr.parse_nested_meta(|meta| {
            let key = &meta.path;
            if key.is_ident("skip") {
                if parsed.skip {
                    return Err(meta.error("`skip` is set twice"));
                }
                parsed.skip = true;
                skip_span = Some(key.span());
                Ok(())
            } else if key.is_ident("name") {
                if parsed.label.is_some() {
                    return Err(meta.error("`name` is set twice"));
                }
                parsed.label = Some(meta.value()?.parse()?);
                Ok(())
            } else if key.is_ident("min") {
                if parsed.min.is_some() {
                    return Err(meta.error("`min` is set twice"));
                }
                parsed.min = Some((number(meta.value()?)?, key.span()));
                Ok(())
            } else if key.is_ident("max") {
                if parsed.max.is_some() {
                    return Err(meta.error("`max` is set twice"));
                }
                parsed.max = Some((number(meta.value()?)?, key.span()));
                Ok(())
            } else if key.is_ident("step") {
                if parsed.step.is_some() {
                    return Err(meta.error("`step` is set twice"));
                }
                parsed.step = Some(number(meta.value()?)?);
                Ok(())
            } else {
                Err(meta.error(
                    "unknown `reflect` attribute; the field attributes are \
                     `skip`, `name`, `min`, `max` and `step`",
                ))
            }
        })?;
    }

    check(&parsed, skip_span)?;
    Ok(parsed)
}

/// The combinations that parse and do not mean anything.
fn check(parsed: &FieldAttrs, skip_span: Option<Span>) -> syn::Result<()> {
    if let Some(span) = skip_span
        && (parsed.label.is_some()
            || parsed.min.is_some()
            || parsed.max.is_some()
            || parsed.step.is_some())
    {
        return Err(syn::Error::new(
            span,
            "a skipped field has no row, so it can carry no `name`, `min`, `max` or `step`",
        ));
    }

    match (parsed.min, parsed.max) {
        (Some((min, _)), Some((max, span))) if min > max => Err(syn::Error::new(
            span,
            "`max` is below `min`, so the range admits nothing",
        )),
        (Some((_, span)), None) => Err(syn::Error::new(
            span,
            "`min` needs a `max`: a range is the pair, and a widget cannot clamp one end",
        )),
        (None, Some((_, span))) => Err(syn::Error::new(
            span,
            "`max` needs a `min`: a range is the pair, and a widget cannot clamp one end",
        )),
        _ => Ok(()),
    }
}

/// One number after a `=`, negative sign and all.
///
/// Written against the token stream rather than `syn::Expr` because `-1.0` is a
/// unary expression and not a literal, and a `syn` built without the `full`
/// feature parses no expression grammar to speak of. Integers are accepted so
/// that `min = 0` need not be written `0.0`.
fn number(input: ParseStream<'_>) -> syn::Result<f64> {
    let negative = input.peek(Token![-]);
    if negative {
        input.parse::<Token![-]>()?;
    }
    let lit: Lit = input.parse()?;
    let magnitude = match &lit {
        Lit::Float(value) => value.base10_parse::<f64>()?,
        Lit::Int(value) => value.base10_parse::<f64>()?,
        other => {
            return Err(syn::Error::new(other.span(), "expected a number"));
        }
    };
    Ok(if negative { -magnitude } else { magnitude })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;
    use syn::{DeriveInput, parse_quote};

    /// The attributes of `input`'s first field.
    fn first_field(input: &DeriveInput) -> syn::Result<FieldAttrs> {
        let syn::Data::Struct(data) = &input.data else {
            unreachable!("the fixtures are all structs")
        };
        field(&data.fields.iter().next().expect("one field").attrs)
    }

    /// The message a refusal carries.
    ///
    /// `expect_err` is not reachable here: it wants `Debug` on the success side,
    /// and neither of these types has one — see [`Container`].
    fn refusal<T>(result: syn::Result<T>, expectation: &str) -> String {
        match result {
            Ok(_) => panic!("expected a refusal: {expectation}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_type_with_no_attribute_reaches_this_crate_by_its_own_name() {
        let input: DeriveInput = parse_quote! { struct S { a: f32 } };
        let krate = container(&input.attrs).expect("no attribute parses").krate;
        assert_eq!(krate.to_token_stream().to_string(), ":: crcbl_reflect");
    }

    #[test]
    fn the_crate_attribute_redirects_the_paths_the_derive_writes() {
        let input: DeriveInput = parse_quote! {
            #[reflect(crate = "crcbl::reflect")]
            struct S { a: f32 }
        };
        let krate = container(&input.attrs).expect("a path parses").krate;
        assert_eq!(krate.to_token_stream().to_string(), "crcbl :: reflect");
    }

    #[test]
    fn an_unknown_container_key_is_refused_by_name() {
        let input: DeriveInput = parse_quote! {
            #[reflect(rename_all = "camelCase")]
            struct S { a: f32 }
        };
        let error = refusal(container(&input.attrs), "the key is not one of ours");
        assert!(error.contains("unknown `reflect` attribute"), "{error}");
    }

    #[test]
    fn the_whole_field_vocabulary_parses() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[reflect(name = "Elevation", min = -1.0, max = 1, step = 0.01)]
                a: f32,
            }
        };
        let parsed = first_field(&input).expect("every key is one of ours");
        assert!(!parsed.skip);
        assert_eq!(
            parsed.label.map(|lit| lit.value()),
            Some("Elevation".to_owned())
        );
        assert_eq!(parsed.min.map(|(value, _)| value), Some(-1.0));
        assert_eq!(parsed.max.map(|(value, _)| value), Some(1.0));
        assert_eq!(parsed.step, Some(0.01));
    }

    #[test]
    fn a_bare_skip_parses_and_carries_nothing_else() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[reflect(skip)]
                a: f32,
            }
        };
        let parsed = first_field(&input).expect("`skip` alone parses");
        assert!(parsed.skip);
        assert!(parsed.label.is_none());
    }

    #[test]
    fn a_skipped_field_may_not_also_be_labelled() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[reflect(skip, name = "Hidden")]
                a: f32,
            }
        };
        let error = refusal(first_field(&input), "the pair means nothing");
        assert!(error.contains("a skipped field has no row"), "{error}");
    }

    #[test]
    fn half_a_range_is_refused_from_either_end() {
        let low: DeriveInput = parse_quote! {
            struct S {
                #[reflect(min = 0.0)]
                a: f32,
            }
        };
        let error = refusal(first_field(&low), "a lone `min`");
        assert!(error.contains("`min` needs a `max`"), "{error}");

        let high: DeriveInput = parse_quote! {
            struct S {
                #[reflect(max = 1.0)]
                a: f32,
            }
        };
        let error = refusal(first_field(&high), "a lone `max`");
        assert!(error.contains("`max` needs a `min`"), "{error}");
    }

    #[test]
    fn an_inverted_range_is_refused() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[reflect(min = 1.0, max = 0.0)]
                a: f32,
            }
        };
        let error = refusal(first_field(&input), "nothing is in that range");
        assert!(error.contains("`max` is below `min`"), "{error}");
    }

    #[test]
    fn a_key_set_twice_is_refused() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[reflect(name = "One", name = "Two")]
                a: f32,
            }
        };
        let error = refusal(first_field(&input), "which one would win?");
        assert!(error.contains("`name` is set twice"), "{error}");
    }

    #[test]
    fn an_unknown_field_key_lists_the_ones_that_exist() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[reflect(tooltip = "hi")]
                a: f32,
            }
        };
        let message = refusal(first_field(&input), "`tooltip` is not one of ours");
        assert!(message.contains("`skip`"), "{message}");
        assert!(message.contains("`step`"), "{message}");
    }

    #[test]
    fn a_range_end_that_is_not_a_number_is_refused() {
        let input: DeriveInput = parse_quote! {
            struct S {
                #[reflect(min = "low", max = 1.0)]
                a: f32,
            }
        };
        let error = refusal(first_field(&input), "a string is not a bound");
        assert!(error.contains("expected a number"), "{error}");
    }
}
