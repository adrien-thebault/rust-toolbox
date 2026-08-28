//! The upstream crates this toolbox was built against.
//!
//! Depending on `toolbox::deps::axum` rather than declaring `axum` yourself
//! guarantees you link the version the toolbox's types come from. Two versions
//! of axum in one binary produce errors about `Router` not being `Router`.
//!
//! # What cannot be re-exported, and why
//!
//! **Re-export crates whose types you hand out; you cannot re-export crates
//! whose macros consumers invoke.** `diesel`, `serde` and `prost` are all in
//! the second group: their derives emit absolute paths like `diesel::` and
//! `serde::` into your crate, which only resolve if the crate is a direct
//! dependency under that exact name. This was measured rather than assumed - a
//! consumer using only the re-export gets 41 compile errors.
//!
//! So those three stay in the consumer's own manifest.
//! The top-level README carries the fragment to copy, and `cargo deny check bans` makes a duplicate a
//! red build rather than a confusing one.

pub use http;
#[cfg(feature = "grpc")]
pub use tonic;
pub use tower;
pub use tower_http;
#[cfg(feature = "web")]
pub use {axum, axum_extra};
