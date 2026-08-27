//! `#[derive(Entity)]`: parsing the attribute, then generating the methods.
//!
//! The two halves are split because they fail differently: `parse` produces
//! `syn::Error`s aimed at a span in the caller's source, `expand` produces
//! tokens and cannot fail at all.

// Codegen and attribute parsing are each one long straight-line function by
// nature; splitting them to satisfy a line count would only scatter them.
#![allow(clippy::too_many_lines)]

mod expand;
mod parse;

use proc_macro2::TokenStream;
use syn::DeriveInput;

/// Expand one `#[derive(Entity)]`, or the compile error explaining why not.
///
/// # Arguments
///
/// * `input` - The parsed struct. Everything the expansion needs comes from it
///   and from its `#[entity(..)]` attribute.
pub fn derive(input: &DeriveInput) -> TokenStream {
    match parse::parse(input) {
        Ok(cfg) => expand::expand(&cfg),
        Err(e) => e.to_compile_error(),
    }
}
