//! Turning a parsed [`Config`] into the `impl` blocks.
//!
//! Every variant gets an explicit match arm in all three methods, so there is
//! no wildcard and thus no `unreachable_patterns` warning when a consumer
//! classifies every variant.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::parse::{Classify, Config, FieldShape, MetaSource, SingleField, Variant, single_field};

/// Build the `impl ServiceError` (and optionally `impl From<_> for
/// tonic::Status`) for `cfg`.
pub fn expand(cfg: &Config) -> TokenStream {
    let ident = &cfg.ident;
    let (impl_generics, ty_generics, where_clause) = cfg.generics.split_for_impl();
    let domain = &cfg.domain;

    let code_arms = cfg.variants.iter().map(code_arm);
    let kind_arms = cfg.variants.iter().map(kind_arm);
    let metadata_arms = cfg.variants.iter().map(metadata_arm);

    let service_error_impl = quote! {
        #[automatically_derived]
        impl #impl_generics ::toolbox_core::ServiceError for #ident #ty_generics #where_clause {
            fn code(&self) -> &'static str {
                match self { #(#code_arms),* }
            }

            fn domain(&self) -> &'static str {
                #domain
            }

            fn kind(&self) -> ::toolbox_core::ErrorKind {
                match self { #(#kind_arms),* }
            }

            fn metadata(&self) -> ::std::collections::BTreeMap<::std::string::String, ::std::string::String> {
                match self { #(#metadata_arms),* }
            }
        }
    };

    let status_impl = cfg.status.then(|| {
        quote! {
            #[automatically_derived]
            impl #impl_generics ::core::convert::From<#ident #ty_generics> for ::tonic::Status #where_clause {
                fn from(err: #ident #ty_generics) -> Self {
                    ::toolbox_grpc::to_status(err)
                }
            }
        }
    });

    quote! {
        #service_error_impl
        #status_impl
    }
}

/// The minimal pattern that only discriminates the variant.
fn discriminant_pattern(variant: &Variant) -> TokenStream {
    let ident = &variant.ident;
    match &variant.fields {
        FieldShape::Unit => quote!(Self::#ident),
        FieldShape::Tuple(_) => quote!(Self::#ident(..)),
        FieldShape::Named(_) => quote!(Self::#ident { .. }),
    }
}

/// `Pattern => "CODE"`, or delegation for a transparent variant.
fn code_arm(variant: &Variant) -> TokenStream {
    match &variant.classify {
        Classify::Transparent => {
            let (pat, binding) = transparent_pattern(variant);
            quote!(#pat => ::toolbox_core::ServiceError::code(#binding))
        }
        Classify::Explicit { code, .. } => {
            let pat = discriminant_pattern(variant);
            quote!(#pat => #code)
        }
    }
}

/// `Pattern => ErrorKind::_`, or delegation for a transparent variant.
fn kind_arm(variant: &Variant) -> TokenStream {
    match &variant.classify {
        Classify::Transparent => {
            let (pat, binding) = transparent_pattern(variant);
            quote!(#pat => ::toolbox_core::ServiceError::kind(#binding))
        }
        Classify::Explicit { kind, .. } => {
            let pat = discriminant_pattern(variant);
            quote!(#pat => ::toolbox_core::ErrorKind::#kind)
        }
    }
}

/// `Pattern => <map>`: delegation, an empty map, or one built from `meta(..)`.
fn metadata_arm(variant: &Variant) -> TokenStream {
    match &variant.classify {
        Classify::Transparent => {
            let (pat, binding) = transparent_pattern(variant);
            quote!(#pat => ::toolbox_core::ServiceError::metadata(#binding))
        }
        Classify::Explicit { meta, .. } if meta.is_empty() => {
            let pat = discriminant_pattern(variant);
            quote!(#pat => ::std::collections::BTreeMap::new())
        }
        Classify::Explicit { meta, .. } => {
            let ident = &variant.ident;
            let inserts = meta.iter().map(|entry| {
                let key = entry.key.to_string();
                let value = match &entry.source {
                    MetaSource::Index(i) => {
                        let binder = format_ident!("__f{}", i);
                        quote!(#binder)
                    }
                    MetaSource::Field(f) => quote!(#f),
                };
                quote! {
                    __m.insert(
                        ::std::string::String::from(#key),
                        ::std::string::ToString::to_string(#value),
                    );
                }
            });
            let pat = match &variant.fields {
                FieldShape::Tuple(n) => {
                    let used: std::collections::BTreeSet<usize> = meta
                        .iter()
                        .filter_map(|e| match e.source {
                            MetaSource::Index(i) => Some(i),
                            MetaSource::Field(_) => None,
                        })
                        .collect();
                    let slots = (0..*n).map(|i| {
                        if used.contains(&i) {
                            let binder = format_ident!("__f{}", i);
                            quote!(#binder)
                        } else {
                            quote!(_)
                        }
                    });
                    quote!(Self::#ident(#(#slots),*))
                }
                FieldShape::Named(_) => {
                    let names = meta.iter().filter_map(|e| match &e.source {
                        MetaSource::Field(f) => Some(f),
                        MetaSource::Index(_) => None,
                    });
                    quote!(Self::#ident { #(#names),*, .. })
                }
                // check_meta_source rejects `meta` on a unit variant before here.
                FieldShape::Unit => quote!(Self::#ident),
            };
            quote! {
                #pat => {
                    let mut __m = ::std::collections::BTreeMap::new();
                    #(#inserts)*
                    __m
                }
            }
        }
    }
}

/// The pattern that binds a transparent variant's one field as `__inner`, and
/// that binding.
fn transparent_pattern(variant: &Variant) -> (TokenStream, TokenStream) {
    let ident = &variant.ident;
    match single_field(&variant.fields) {
        Some(SingleField::Index) => (quote!(Self::#ident(__inner)), quote!(__inner)),
        Some(SingleField::Named(field)) => {
            (quote!(Self::#ident { #field: __inner }), quote!(__inner))
        }
        // parse rejects `transparent` on any other shape before here.
        None => (quote!(Self::#ident { .. }), quote!(unreachable!())),
    }
}
