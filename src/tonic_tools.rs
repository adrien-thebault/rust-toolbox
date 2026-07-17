//! a `ServiceError` trait + `tonic::Status` <-> `google.rpc.ErrorInfo`
//! conversion, so a gRPC service's own domain error can flow across the
//! wire carrying a structured code/domain/metadata instead of collapsing
//! into a bare status code and string message. Usable by any tonic service
//! crate, not just an axum gateway - see `axum_tools::api_error` for the
//! HTTP-facing decode side that builds on this.
mod service_error;
pub use service_error::*;

/// propagates the ambient request id (see `tower_tools::layers::request_id`)
/// onto outgoing gRPC calls. Requires the `tower` feature too, since it
/// builds on `request_id_context_layer`'s ambient context.
#[cfg(feature = "tower")]
mod request_id;
#[cfg(feature = "tower")]
pub use request_id::*;
