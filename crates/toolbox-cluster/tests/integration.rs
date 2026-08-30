//! One harness per crate; the module tree mirrors `src/`.
#![allow(missing_docs, clippy::missing_panics_doc, clippy::missing_errors_doc)]

mod deployment;
mod event;
mod kv;
mod lock;
