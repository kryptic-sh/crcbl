//! Turning a `#[derive(Reflect)]` input into the `impl`.
//!
//! The five interesting methods are built separately and assembled at the end,
//! because a struct and an enum differ in all five and in nothing else.

use proc_macro2::{Literal, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Data, DataEnum, DeriveInput, Fields, Ident, Index, Member, Path, Type};

use crate::attrs;

/// The five method bodies that differ between a struct and an enum, plus
/// whether `index` is read by any of them.
struct Body {
    kind: TokenStream,
    fields: TokenStream,
    field: TokenStream,
    field_mut: TokenStream,
    variant: TokenStream,
    /// The per-field `Reflect` assertions, which name the field in the refusal.
    assertions: TokenStream,
    reads_index: bool,
}

/// One row of an inspector: a field that survived `#[reflect(skip)]`.
struct Row<'a> {
    /// How the value is reached from `self`, for a struct.
    member: Member,
    /// What the binding is called, for an enum's pattern.
    binding: Ident,
    /// The field's type, whose span is where a missing `Reflect` is reported.
    ty: &'a Type,
    /// The `crcbl_reflect::Field` literal this row expands to.
    literal: TokenStream,
}

/// The whole `impl`, or the first refusal.
pub(crate) fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let krate = attrs::container(&input.attrs)?.krate;
    let ident = &input.ident;
    let name = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let body = match &input.data {
        Data::Struct(data) => struct_body(&krate, &data.fields)?,
        Data::Enum(data) => enum_body(&krate, ident, data)?,
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "`#[derive(Reflect)]` does not describe a union: which field is live is \
                 not knowable from the value, and reading one is unsafe",
            ));
        }
    };

    let Body {
        kind,
        fields,
        field,
        field_mut,
        variant,
        assertions,
        reads_index,
    } = body;
    // A generic type's fields are spelled in its own parameters, which an
    // anonymous const cannot name; there, the bound is still enforced by the
    // expansion's own coercion and the refusal names the type rather than the
    // field. `#[derive(Reflect)]`'s own docs say so.
    let assertions = if input.generics.params.is_empty() {
        assertions
    } else {
        TokenStream::new()
    };
    let index = if reads_index {
        format_ident!("index")
    } else {
        format_ident!("_index")
    };

    Ok(quote! {
        #assertions

        #[automatically_derived]
        impl #impl_generics #krate::Reflect for #ident #ty_generics #where_clause {
            fn type_name(&self) -> &'static str {
                #name
            }

            fn kind(&self) -> #krate::Kind {
                #kind
            }

            fn get(&self) -> ::core::option::Option<#krate::Value> {
                ::core::option::Option::None
            }

            fn set(
                &mut self,
                _value: &#krate::Value,
            ) -> ::core::result::Result<(), #krate::SetError> {
                ::core::result::Result::Err(#krate::SetError::NotALeaf { type_name: #name })
            }

            fn fields(&self) -> &'static [#krate::Field] {
                #fields
            }

            fn field(&self, #index: usize) -> ::core::option::Option<&dyn #krate::Reflect> {
                #field
            }

            fn field_mut(
                &mut self,
                #index: usize,
            ) -> ::core::option::Option<&mut dyn #krate::Reflect> {
                #field_mut
            }

            fn variant(&self) -> ::core::option::Option<&'static str> {
                #variant
            }

            fn as_any(&self) -> &dyn ::core::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any {
                self
            }
        }
    })
}

/// The rows of one field list, in source order, minus the skipped ones.
fn rows<'a>(krate: &Path, fields: &'a Fields) -> syn::Result<Vec<Row<'a>>> {
    let mut rows = Vec::new();
    for (position, field) in fields.iter().enumerate() {
        let parsed = attrs::field(&field.attrs)?;
        if parsed.skip {
            continue;
        }

        let (member, binding, name) = match &field.ident {
            Some(ident) => (
                Member::Named(ident.clone()),
                ident.clone(),
                ident.to_string(),
            ),
            None => (
                Member::Unnamed(Index::from(position)),
                format_ident!("__field{}", position),
                position.to_string(),
            ),
        };

        let label = parsed
            .label
            .as_ref()
            .map_or_else(|| name.clone(), syn::LitStr::value);

        let range = match (parsed.min, parsed.max) {
            (Some((min, _)), Some((max, _))) => {
                let min = Literal::f64_suffixed(min);
                let max = Literal::f64_suffixed(max);
                quote!(::core::option::Option::Some(#krate::Range { min: #min, max: #max }))
            }
            // `attrs::field` has already refused a half-written range.
            _ => quote!(::core::option::Option::None),
        };

        let step = parsed.step.map_or_else(
            || quote!(::core::option::Option::None),
            |step| {
                let step = Literal::f64_suffixed(step);
                quote!(::core::option::Option::Some(#step))
            },
        );

        rows.push(Row {
            member,
            binding,
            ty: &field.ty,
            literal: quote! {
                #krate::Field {
                    name: #name,
                    label: #label,
                    range: #range,
                    step: #step,
                }
            },
        });
    }
    Ok(rows)
}

/// One `const fn` per field, called from a `const` so that a field whose type is
/// not `Reflect` is refused with **that field's name in the message**.
///
/// The expansion's own `&Field -> &dyn Reflect` coercion already makes it an
/// error rather than a missing row, but rustc reports that at
/// `#[derive(Reflect)]` — the one place that tells an author nothing about which
/// field is wrong. A named bound puts the field in the text: *required by a
/// bound in `assert_field_opaque_is_reflect`*.
///
/// `const fn` called from `const _` rather than a plain `fn`, because an
/// uncalled function is dead code and this workspace builds with `-D warnings`;
/// the call is what makes it live, and it evaluates to nothing.
///
/// Each group gets its own nested block so that two enum variants may each have
/// a field called `width` without the two assertions colliding.
fn assertions(krate: &Path, groups: &[Vec<Row<'_>>]) -> TokenStream {
    let blocks = groups.iter().map(|rows| {
        let checks = rows.iter().map(|row| {
            let name = format_ident!("assert_field_{}_is_reflect", row.binding);
            let ty = row.ty;
            quote_spanned! { ty.span() =>
                const fn #name<T: #krate::Reflect + ?Sized>() {}
                const _: () = #name::<#ty>();
            }
        });
        quote!(const _: () = { #(#checks)* };)
    });
    quote!(#(#blocks)*)
}

/// `&[Field]` for a row list: a `const` so the slice is `'static` without the
/// impl having to name a place to put it.
fn field_slice(krate: &Path, rows: &[Row<'_>]) -> TokenStream {
    if rows.is_empty() {
        return quote!(&[]);
    }
    let literals = rows.iter().map(|row| &row.literal);
    quote! {
        {
            const FIELDS: &[#krate::Field] = &[#(#literals),*];
            FIELDS
        }
    }
}

/// `match index { … }` over a row list, given how each row's value is spelled.
///
/// The arm is spanned at the field's **type**, so a field whose type has no
/// `Reflect` impl is reported there — at the field, in the type's own
/// definition, rather than inside an expansion the author never wrote.
fn index_match(
    krate: &Path,
    rows: &[Row<'_>],
    mutable: bool,
    value: impl Fn(&Row<'_>) -> TokenStream,
) -> TokenStream {
    if rows.is_empty() {
        return quote!(::core::option::Option::None);
    }
    let arms = rows.iter().enumerate().map(|(position, row)| {
        let position = Literal::usize_suffixed(position);
        let value = value(row);
        // The **cast** is what has to carry the field's span, not just the
        // `Some` around it: the coercion `&Field -> &dyn Reflect` is where the
        // missing impl is discovered, and left to the return type it is
        // discovered at `#[derive(Reflect)]` instead — which is the one place
        // that tells an author nothing about which field is wrong.
        let target = if mutable {
            quote!(&mut dyn #krate::Reflect)
        } else {
            quote!(&dyn #krate::Reflect)
        };
        let some = quote_spanned! { row.ty.span() =>
            ::core::option::Option::Some(#value as #target)
        };
        quote!(#position => #some)
    });
    quote! {
        match index {
            #(#arms,)*
            _ => ::core::option::Option::None,
        }
    }
}

/// The five bodies for a struct, a tuple struct or a unit struct.
fn struct_body(krate: &Path, fields: &Fields) -> syn::Result<Body> {
    let rows = rows(krate, fields)?;
    let slice = field_slice(krate, &rows);
    let field = index_match(krate, &rows, false, |row| {
        let member = &row.member;
        quote!(&self.#member)
    });
    let field_mut = index_match(krate, &rows, true, |row| {
        let member = &row.member;
        quote!(&mut self.#member)
    });

    let reads_index = !rows.is_empty();
    let assertions = assertions(krate, ::core::slice::from_ref(&rows));

    Ok(Body {
        kind: quote!(#krate::Kind::Struct),
        fields: slice,
        field,
        field_mut,
        variant: quote!(::core::option::Option::None),
        assertions,
        reads_index,
    })
}

/// The five bodies for an enum.
///
/// Every one of them is a `match self` first: which rows exist, and what they
/// are, is a property of the **active variant** rather than of the type.
fn enum_body(krate: &Path, ident: &Ident, data: &DataEnum) -> syn::Result<Body> {
    if data.variants.is_empty() {
        return Err(syn::Error::new(
            ident.span(),
            "`#[derive(Reflect)]` needs an enum with at least one variant: a value of \
             this type cannot exist, so there is nothing for a panel to show",
        ));
    }

    let mut wildcard_arms = Vec::new();
    let mut variant_arms = Vec::new();
    let mut field_arms = Vec::new();
    let mut field_mut_arms = Vec::new();
    let mut groups = Vec::new();
    let mut reads_index = false;

    for variant in &data.variants {
        let vident = &variant.ident;
        let vname = vident.to_string();
        let rows = rows(krate, &variant.fields)?;
        reads_index |= !rows.is_empty();

        // `Self::V { .. }` / `Self::V(..)` / `Self::V`, for the three bodies
        // that only need to know which variant is active.
        let wildcard = match &variant.fields {
            Fields::Named(_) => quote!(Self::#vident { .. }),
            Fields::Unnamed(_) => quote!(Self::#vident(..)),
            Fields::Unit => quote!(Self::#vident),
        };
        let slice = field_slice(krate, &rows);
        wildcard_arms.push(quote!(#wildcard => #slice));
        variant_arms.push(quote!(#wildcard => ::core::option::Option::Some(#vname)));

        // The binding pattern: every surviving field named, every skipped one
        // matched and dropped.
        let bound = binding_pattern(vident, &variant.fields, &rows);
        let binding = |row: &Row<'_>| {
            let binding = &row.binding;
            quote!(#binding)
        };
        let field = index_match(krate, &rows, false, binding);
        let field_mut = index_match(krate, &rows, true, binding);
        field_arms.push(quote!(#bound => #field));
        field_mut_arms.push(quote!(#bound => #field_mut));
        groups.push(rows);
    }

    let assertions = assertions(krate, &groups);

    Ok(Body {
        kind: quote!(#krate::Kind::Enum),
        fields: quote!(match self { #(#wildcard_arms,)* }),
        field: quote!(match self { #(#field_arms,)* }),
        field_mut: quote!(match self { #(#field_mut_arms,)* }),
        variant: quote!(match self { #(#variant_arms,)* }),
        assertions,
        reads_index,
    })
}

/// `Self::V { a, .. }` or `Self::V(__field0, _)`, binding exactly the rows.
///
/// A named variant takes `..` for the skipped remainder; an unnamed one has to
/// list every position, because a tuple pattern cannot say "the rest" twice.
fn binding_pattern(vident: &Ident, fields: &Fields, rows: &[Row<'_>]) -> TokenStream {
    match fields {
        Fields::Unit => quote!(Self::#vident),
        Fields::Named(_) => {
            let bound = rows.iter().map(|row| &row.binding);
            quote!(Self::#vident { #(#bound,)* .. })
        }
        Fields::Unnamed(unnamed) => {
            let positions = (0..unnamed.unnamed.len()).map(|position| {
                rows.iter()
                    .find(|row| {
                        matches!(&row.member, Member::Unnamed(index)
                        if index.index as usize == position)
                    })
                    .map_or_else(|| quote!(_), |row| row.binding.to_token_stream())
            });
            quote!(Self::#vident(#(#positions,)*))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    /// The expansion, whitespace-normalised so a substring check is about the
    /// tokens rather than about how `quote!` spaced them.
    fn expand(input: DeriveInput) -> String {
        derive(&input)
            .expect("the fixture derives")
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_named_struct_expands_to_one_row_per_field_in_source_order() {
        let text = expand(parse_quote! {
            struct Brick {
                position: [f64; 3],
                half_extents: [f64; 3],
            }
        });
        assert!(
            text.contains(":: crcbl_reflect :: Kind :: Struct"),
            "{text}"
        );
        assert!(text.contains("name : \"position\""), "{text}");
        assert!(text.contains("name : \"half_extents\""), "{text}");
        assert!(
            text.contains(
                "0usize => :: core :: option :: Option :: Some \
                 (& self . position as & dyn :: crcbl_reflect :: Reflect)"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "1usize => :: core :: option :: Option :: Some \
                 (& self . half_extents as & dyn :: crcbl_reflect :: Reflect)"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "0usize => :: core :: option :: Option :: Some \
                 (& mut self . position as & mut dyn :: crcbl_reflect :: Reflect)"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_skipped_field_leaves_no_row_and_closes_the_gap_behind_it() {
        let text = expand(parse_quote! {
            struct Sun {
                elevation: f32,
                #[reflect(skip)]
                cached: f32,
                intensity: f32,
            }
        });
        assert!(!text.contains("\"cached\""), "{text}");
        assert!(!text.contains("self . cached"), "{text}");
        // `intensity` is the second row even though it is the third field.
        assert!(
            text.contains("1usize => :: core :: option :: Option :: Some (& self . intensity as"),
            "{text}"
        );
        assert!(
            !text.contains("assert_field_cached_is_reflect"),
            "a skipped field is not held to the trait either: {text}"
        );
    }

    #[test]
    fn the_attributes_land_in_the_row_literal() {
        let text = expand(parse_quote! {
            struct Sun {
                #[reflect(name = "Elevation", min = -1.0, max = 1.0, step = 0.01)]
                elevation: f32,
            }
        });
        assert!(text.contains("label : \"Elevation\""), "{text}");
        assert!(text.contains("min : - 1f64"), "{text}");
        assert!(text.contains("max : 1f64"), "{text}");
        assert!(text.contains("Some (0.01f64)"), "{text}");
    }

    #[test]
    fn a_tuple_struct_names_its_rows_by_position() {
        let text = expand(parse_quote! { struct Metres(f64); });
        assert!(text.contains("name : \"0\""), "{text}");
        assert!(text.contains("Some (& self . 0 as & dyn"), "{text}");
    }

    #[test]
    fn a_unit_struct_has_no_rows_and_does_not_read_the_index() {
        let text = expand(parse_quote! { struct Marker; });
        assert!(
            text.contains("fn field (& self , _index : usize)"),
            "{text}"
        );
        assert!(
            text.contains("fn fields (& self) -> & 'static [:: crcbl_reflect :: Field] { & [] }"),
            "{text}"
        );
    }

    #[test]
    fn an_enum_matches_the_active_variant_in_every_body() {
        let text = expand(parse_quote! {
            enum Shape {
                Platform { width: f64, depth: f64 },
                Dome(f64),
                Flat,
            }
        });
        assert!(text.contains(":: crcbl_reflect :: Kind :: Enum"), "{text}");
        assert!(
            text.contains(
                "Self :: Platform { .. } => :: core :: option :: Option :: Some (\"Platform\")"
            ),
            "{text}"
        );
        assert!(
            text.contains("Self :: Dome (..) => :: core :: option :: Option :: Some (\"Dome\")"),
            "{text}"
        );
        assert!(
            text.contains("Self :: Flat => :: core :: option :: Option :: Some (\"Flat\")"),
            "{text}"
        );
        assert!(
            text.contains("Self :: Platform { width , depth , .. }"),
            "{text}"
        );
        assert!(text.contains("Self :: Dome (__field0 ,)"), "{text}");
    }

    #[test]
    fn a_skipped_field_of_a_tuple_variant_keeps_its_position_in_the_pattern() {
        let text = expand(parse_quote! {
            enum Shape {
                Dome(#[reflect(skip)] u32, f64),
            }
        });
        assert!(text.contains("Self :: Dome (_ , __field1 ,)"), "{text}");
        assert!(text.contains("name : \"1\""), "{text}");
    }

    #[test]
    fn the_crate_attribute_redirects_every_path_the_expansion_writes() {
        let text = expand(parse_quote! {
            #[reflect(crate = "crcbl::reflect")]
            struct Brick { position: [f64; 3] }
        });
        assert!(
            text.contains("impl crcbl :: reflect :: Reflect for Brick"),
            "{text}"
        );
        assert!(!text.contains(":: crcbl_reflect ::"), "{text}");
    }

    #[test]
    fn a_union_is_refused_with_the_reason() {
        let error = derive(&parse_quote! {
            union Raw { a: u32, b: f32 }
        })
        .expect_err("a union has no knowable live field");
        assert!(
            error.to_string().contains("does not describe a union"),
            "{error}"
        );
    }

    #[test]
    fn an_enum_with_no_variants_is_refused() {
        let error = derive(&parse_quote! { enum Never {} }).expect_err("no value of it can exist");
        assert!(
            error.to_string().contains("at least one variant"),
            "{error}"
        );
    }

    #[test]
    fn every_row_is_held_to_the_trait_by_a_bound_that_names_the_field() {
        let text = expand(parse_quote! {
            struct Brick { position: [f64; 3], half_extents: [f64; 3] }
        });
        assert!(
            text.contains(
                "const fn assert_field_position_is_reflect \
                 < T : :: crcbl_reflect :: Reflect + ? Sized > () { }"
            ),
            "{text}"
        );
        assert!(
            text.contains("const _ : () = assert_field_position_is_reflect :: < [f64 ; 3] > () ;"),
            "{text}"
        );
        assert!(
            text.contains("assert_field_half_extents_is_reflect"),
            "{text}"
        );
    }

    #[test]
    fn each_variant_gets_its_own_assertion_block_so_two_widths_do_not_collide() {
        let text = expand(parse_quote! {
            enum Shape {
                Platform { width: f64 },
                Ramp { width: f32 },
            }
        });
        assert_eq!(
            text.matches("const fn assert_field_width_is_reflect")
                .count(),
            2,
            "one per variant, each in its own block: {text}"
        );
    }

    #[test]
    fn a_generic_type_gets_no_named_assertion_because_a_const_cannot_name_a_parameter() {
        let text = expand(parse_quote! {
            struct Pair<T: Copy> { left: T, right: T }
        });
        assert!(!text.contains("assert_field_left_is_reflect"), "{text}");
    }

    #[test]
    fn a_generic_type_carries_its_own_bounds_into_the_impl() {
        let text = expand(parse_quote! {
            struct Pair<T: Copy> { left: T, right: T }
        });
        assert!(
            text.contains("impl < T : Copy > :: crcbl_reflect :: Reflect for Pair < T >"),
            "{text}"
        );
    }
}
