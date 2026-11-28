//! Turning the checked [`Shape`] into the three impls: `Display`, `Error`, and
//! a `From` per `#[from]` field.
//!
//! Every path the expansion names is absolute and in `core` (`::core::fmt`,
//! `::core::error::Error`, which `std::error::Error` re-exports), so it expands
//! the same whatever the deriving module imports, and in a `no_std` crate.

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{DeriveInput, Ident};

use crate::attrs::{self, Field, Message, Shape, Style, Variant};
use crate::fmt;

/// The whole expansion, or the first refusal.
pub(crate) fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let shape = attrs::read(input)?;
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let display = display_body(&shape)?;
    let source = source_method(&shape);
    let froms = shape.variants.iter().filter_map(|variant| {
        let field = variant.fields.iter().find(|field| field.from.is_some())?;
        let ty = field.ty;
        let value = construct(variant, quote!(source));
        Some(quote! {
            #[automatically_derived]
            impl #impl_generics ::core::convert::From<#ty> for #ident #ty_generics #where_clause {
                fn from(source: #ty) -> Self {
                    #value
                }
            }
        })
    });

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::core::fmt::Display for #ident #ty_generics #where_clause {
            #[allow(unused_variables)]
            fn fmt(&self, __formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                #display
            }
        }

        #[automatically_derived]
        impl #impl_generics ::core::error::Error for #ident #ty_generics #where_clause {
            #source
        }

        #(#froms)*
    })
}

/// `match self { pattern => write, … }`, one arm per variant.
fn display_body(shape: &Shape<'_>) -> syn::Result<TokenStream> {
    if shape.variants.is_empty() {
        // `match self {}` would not be exhaustive: a reference is inhabited
        // even when what it points at cannot be.
        return Ok(quote!(match *self {}));
    }
    let arms = shape
        .variants
        .iter()
        .map(|variant| {
            let pattern = pattern(variant);
            let write = match &variant.message {
                Message::Transparent(_) => {
                    let binding = &variant.fields[0].binding;
                    quote!(::core::fmt::Display::fmt(#binding, __formatter))
                }
                Message::Format { fmt, args } => {
                    let arguments = fmt::arguments(fmt, args, &variant.fields)?;
                    quote!(__formatter.write_fmt(::core::format_args!(#arguments)))
                }
            };
            Ok(quote!(#pattern => #write,))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(quote!(match self { #(#arms)* }))
}

/// `fn source`, or nothing when no variant has a source to return, which
/// leaves the trait's own `None`.
fn source_method(shape: &Shape<'_>) -> TokenStream {
    let any = shape.variants.iter().any(|variant| {
        matches!(variant.message, Message::Transparent(_))
            || variant.fields.iter().any(|field| field.is_source)
    });
    if !any {
        return TokenStream::new();
    }
    let arms = shape.variants.iter().map(|variant| {
        let pattern = pattern(variant);
        let value = if let Message::Transparent(_) = variant.message {
            let field = &variant.fields[0];
            let binding = &field.binding;
            quote_spanned!(field.ty.span()=> ::core::error::Error::source(#binding))
        } else if let Some(field) = variant.fields.iter().find(|field| field.is_source) {
            let binding = &field.binding;
            quote_spanned! {field.ty.span()=>
                ::core::option::Option::Some(#binding as &(dyn ::core::error::Error + 'static))
            }
        } else {
            quote!(::core::option::Option::None)
        };
        quote!(#pattern => #value,)
    });
    quote! {
        #[allow(unused_variables)]
        fn source(&self) -> ::core::option::Option<&(dyn ::core::error::Error + 'static)> {
            match self { #(#arms)* }
        }
    }
}

/// The variant's path: `Self::Name`, or `Self` for a struct.
fn path(variant: &Variant<'_>) -> TokenStream {
    match variant.ident {
        Some(ident) => quote!(Self::#ident),
        None => quote!(Self),
    }
}

/// A pattern binding every field to its [`Field::binding`].
fn pattern(variant: &Variant<'_>) -> TokenStream {
    let path = path(variant);
    let bindings = variant.fields.iter().map(|field| &field.binding);
    match variant.style {
        Style::Named => quote!(#path { #(#bindings),* }),
        Style::Unnamed => quote!(#path ( #(#bindings),* )),
        Style::Unit => path,
    }
}

/// The variant built from `value` as its only field, for `From`.
fn construct(variant: &Variant<'_>, value: TokenStream) -> TokenStream {
    let path = path(variant);
    let field: &Field<'_> = &variant.fields[0];
    match variant.style {
        Style::Named => {
            let name: &Ident = &field.binding;
            quote!(#path { #name: #value })
        }
        _ => quote!(#path ( #value )),
    }
}
