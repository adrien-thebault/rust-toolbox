//! Parsing `#[service_error(..)]`, on the enum and on each variant.
//!
//! Written against `syn` directly, like `entity::parse`: the error spans are
//! part of the contract, so every misuse has a committed `trybuild` case.

use proc_macro2::Span;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Generics, Ident, LitInt, LitStr};

/// Everything the two attribute levels declared, resolved against the enum.
pub struct Config {
    /// The enum the derive is on.
    pub ident: Ident,
    /// Its generics, copied onto the generated impls verbatim.
    pub generics: Generics,
    /// `domain = <expr>`: an expression yielding `&'static str`.
    pub domain: Expr,
    /// Whether `status` was set: also emit `impl From<Self> for tonic::Status`.
    pub status: bool,
    /// One entry per enum variant, in declaration order.
    pub variants: Vec<Variant>,
}

/// One variant and its `#[service_error(..)]`.
pub struct Variant {
    /// The variant name.
    pub ident: Ident,
    /// Its field shape, which decides the match pattern.
    pub fields: FieldShape,
    /// How it classifies.
    pub classify: Classify,
}

/// A variant's fields, only as much as codegen needs.
pub enum FieldShape {
    /// `Variant`.
    Unit,
    /// `Variant(A, B, ..)` - the field count, to size the pattern.
    Tuple(usize),
    /// `Variant { a, b, .. }` - the field names, to bind by name.
    Named(Vec<Ident>),
}

/// How one variant answers `code`, `kind` and `metadata`.
pub enum Classify {
    /// `transparent`: delegate all three to the single inner field's own
    /// `ServiceError` impl.
    Transparent,
    /// An explicit code and kind, plus any `meta(..)` entries.
    Explicit {
        /// `code = "..."`.
        code: LitStr,
        /// `kind = <Ident>`, a `toolbox_core::ErrorKind` variant. Not checked
        /// against the enum's real variants: it is `#[non_exhaustive]`.
        kind: Ident,
        /// `meta(name = <field or index>, ..)`.
        meta: Vec<MetaEntry>,
    },
}

/// One `meta(name = source)` pair.
pub struct MetaEntry {
    /// The metadata key, emitted as a string literal.
    pub key: Ident,
    /// Where its value comes from on the variant.
    pub source: MetaSource,
}

/// Which field a `meta` entry reads.
pub enum MetaSource {
    /// A tuple field, by index.
    Index(usize),
    /// A named field, by name.
    Field(Ident),
}

/// Resolve both attribute levels against the enum.
///
/// # Arguments
///
/// * `input` - The enum carrying `#[derive(ServiceError)]`. A struct, or an
///   enum with no `#[service_error(domain = ..)]`, is rejected here.
pub fn parse(input: &DeriveInput) -> syn::Result<Config> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(ServiceError)] only applies to enums; a service's error type is a set of \
             failures",
        ));
    };

    let attr = input
        .attrs
        .iter()
        .find(|a| a.path().is_ident("service_error"))
        .ok_or_else(|| {
            syn::Error::new(
                input.ident.span(),
                "missing #[service_error(..)]; #[derive(ServiceError)] needs at least \
                 `domain = <expr>`",
            )
        })?;
    let (domain, status) = parse_container(attr)?;

    let variants = data
        .variants
        .iter()
        .map(|v| {
            let fields = field_shape(&v.fields);
            let classify = parse_variant(&v.ident, &v.attrs, &fields)?;
            Ok(Variant {
                ident: v.ident.clone(),
                fields,
                classify,
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    Ok(Config {
        ident: input.ident.clone(),
        generics: input.generics.clone(),
        domain,
        status,
        variants,
    })
}

/// Read the enum-level `#[service_error(domain = <expr> [, status])]`.
fn parse_container(attr: &Attribute) -> syn::Result<(Expr, bool)> {
    let mut domain = None;
    let mut status = false;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("domain") {
            domain = Some(meta.value()?.parse::<Expr>()?);
        } else if meta.path.is_ident("status") {
            status = true;
        } else {
            return Err(meta.error(
                "unknown `service_error` option; the enum level takes `domain = <expr>` and \
                 the bare flag `status`",
            ));
        }
        Ok(())
    })?;
    let domain = domain.ok_or_else(|| {
        syn::Error::new(
            attr.span(),
            "`#[service_error(..)]` on the enum needs `domain = <expr>`, e.g. \
             `domain = crate::SERVICE_ERROR_DOMAIN`",
        )
    })?;
    Ok((domain, status))
}

/// Read one variant's `#[service_error(..)]`.
fn parse_variant(
    variant: &Ident,
    attrs: &[Attribute],
    fields: &FieldShape,
) -> syn::Result<Classify> {
    let attr = attrs.iter().find(|a| a.path().is_ident("service_error"));
    let Some(attr) = attr else {
        return Err(syn::Error::new(
            variant.span(),
            format!(
                "variant `{variant}` has no #[service_error(..)]; add \
                 `#[service_error(code = \"...\", kind = ...)]`, or `#[service_error(transparent)]` \
                 to delegate to an inner error"
            ),
        ));
    };

    let mut transparent = false;
    let mut code: Option<LitStr> = None;
    let mut kind: Option<Ident> = None;
    let mut meta: Vec<MetaEntry> = Vec::new();

    attr.parse_nested_meta(|nested| {
        if nested.path.is_ident("transparent") {
            transparent = true;
        } else if nested.path.is_ident("code") {
            code = Some(nested.value()?.parse::<LitStr>()?);
        } else if nested.path.is_ident("kind") {
            kind = Some(nested.value()?.parse::<Ident>()?);
        } else if nested.path.is_ident("meta") {
            let content;
            syn::parenthesized!(content in nested.input);
            let pairs = content.parse_terminated(parse_meta_entry, syn::Token![,])?;
            meta.extend(pairs);
        } else {
            return Err(nested.error(
                "unknown `service_error` option; a variant takes `transparent`, or \
                 `code = \"...\"` + `kind = ...` with an optional `meta(name = field, ..)`",
            ));
        }
        Ok(())
    })?;

    if transparent {
        if code.is_some() || kind.is_some() || !meta.is_empty() {
            return Err(syn::Error::new(
                attr.span(),
                "`transparent` delegates code, kind and metadata to the inner error; it cannot \
                 be combined with `code`, `kind` or `meta`",
            ));
        }
        if single_field(fields).is_none() {
            return Err(syn::Error::new(
                variant.span(),
                format!(
                    "`transparent` needs variant `{variant}` to have exactly one field, to \
                     delegate to"
                ),
            ));
        }
        return Ok(Classify::Transparent);
    }

    let code = code.ok_or_else(|| {
        syn::Error::new(
            attr.span(),
            format!("variant `{variant}` needs `code = \"SCREAMING_SNAKE_CASE\"`"),
        )
    })?;
    let kind = kind.ok_or_else(|| {
        syn::Error::new(
            attr.span(),
            format!(
                "variant `{variant}` needs `kind = <ErrorKind>`, e.g. `kind = NotFound` or \
                 `kind = Internal`"
            ),
        )
    })?;

    for entry in &meta {
        check_meta_source(variant, fields, &entry.source)?;
    }

    Ok(Classify::Explicit { code, kind, meta })
}

/// Parse one `name = source` inside `meta(..)`.
fn parse_meta_entry(input: syn::parse::ParseStream) -> syn::Result<MetaEntry> {
    let key: Ident = input.parse()?;
    input.parse::<syn::Token![=]>()?;
    let source = if input.peek(LitInt) {
        let lit: LitInt = input.parse()?;
        MetaSource::Index(lit.base10_parse()?)
    } else {
        MetaSource::Field(input.parse()?)
    };
    Ok(MetaEntry { key, source })
}

/// A variant's field shape, from `syn::Fields`.
fn field_shape(fields: &Fields) -> FieldShape {
    match fields {
        Fields::Unit => FieldShape::Unit,
        Fields::Unnamed(u) => FieldShape::Tuple(u.unnamed.len()),
        Fields::Named(n) => {
            FieldShape::Named(n.named.iter().filter_map(|f| f.ident.clone()).collect())
        }
    }
}

/// `Some` binding name for a variant's single field, or `None` when it does
/// not have exactly one. `transparent` needs this.
pub fn single_field(shape: &FieldShape) -> Option<SingleField> {
    match shape {
        FieldShape::Tuple(1) => Some(SingleField::Index),
        FieldShape::Named(names) if names.len() == 1 => Some(SingleField::Named(names[0].clone())),
        _ => None,
    }
}

/// How to bind a `transparent` variant's one field.
pub enum SingleField {
    /// A one-tuple: `Self::V(__inner)`.
    Index,
    /// A one-field struct: `Self::V { name: __inner }`.
    Named(Ident),
}

/// Reject a `meta` source that does not name a real field of the variant.
fn check_meta_source(variant: &Ident, shape: &FieldShape, source: &MetaSource) -> syn::Result<()> {
    match (shape, source) {
        (FieldShape::Tuple(n), MetaSource::Index(i)) if i < n => Ok(()),
        (FieldShape::Named(names), MetaSource::Field(f)) if names.contains(f) => Ok(()),
        (_, MetaSource::Index(i)) => Err(syn::Error::new(
            variant.span(),
            format!("`meta` refers to field {i}, which variant `{variant}` does not have"),
        )),
        (_, MetaSource::Field(f)) => Err(syn::Error::new(
            f.span(),
            format!("`meta` refers to field `{f}`, which variant `{variant}` does not have"),
        )),
    }
}

/// Extension so `Attribute::span()` reads without importing the trait
/// everywhere.
trait AttrSpan {
    /// The attribute's span.
    fn span(&self) -> Span;
}
impl AttrSpan for Attribute {
    fn span(&self) -> Span {
        syn::spanned::Spanned::span(self)
    }
}
