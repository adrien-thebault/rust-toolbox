//! One harness per crate; the module tree mirrors `src/`.
#![allow(missing_docs, clippy::missing_panics_doc)]

mod clock;
mod error;
mod job;
mod scheduler;
mod trigger;
