//! `#[derive(ServiceError)]`: the four `toolbox_core::ServiceError` methods, and
//! optionally `impl From<Self> for tonic::Status`, from one attribute per
//! variant.
//!
//! Split into `parse` and `expand` for the same reason `entity` is: `parse`
//! produces `syn::Error`s aimed at a span in the caller's source, `expand`
//! produces tokens and cannot fail.

mod expand;
mod parse;

use proc_macro2::TokenStream;
use syn::DeriveInput;

/// Expand one `#[derive(ServiceError)]`, or the compile error explaining why
/// not.
///
/// # Arguments
///
/// * `input` - The parsed enum. Every method body is built from its variants
///   and their `#[service_error(..)]` attributes.
pub fn derive(input: &DeriveInput) -> TokenStream {
    match parse::parse(input) {
        Ok(cfg) => expand::expand(&cfg),
        Err(e) => e.to_compile_error(),
    }
}
