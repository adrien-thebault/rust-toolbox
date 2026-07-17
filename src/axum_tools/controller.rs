//! generic scaffolding for composing a gateway's routes out of independent,
//! per-resource pieces instead of one centralized route table.

use axum::Router;

/// implemented by a resource module to build the `Router` for the routes it
/// owns; a gateway composes several of these into its full route table. `S`
/// is the gateway's own state type - this crate doesn't know its shape.
pub trait Controller<S> {
    /// builds the `Router` for the routes this controller owns
    fn router(&self) -> Router<S>;
}
