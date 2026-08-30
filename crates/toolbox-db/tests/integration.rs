//! One harness per crate; the module tree mirrors `src/`.
// Test scaffolding is not public API.
#![allow(missing_docs, clippy::missing_panics_doc, clippy::missing_errors_doc)]

pub mod fixtures;

mod db;
mod derive;
mod generic_backend;
mod pagination;
