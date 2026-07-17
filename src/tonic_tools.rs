//! a `ServiceError` trait + `tonic::Status` <-> `google.rpc.ErrorInfo`
//! conversion, so a gRPC service's own domain error can flow across the
//! wire carrying a structured code/domain/metadata instead of collapsing
//! into a bare status code and string message. Usable by any tonic service
//! crate, not just an axum gateway - see `axum_tools::api_error` for the
//! HTTP-facing decode side that builds on this.

mod service_error;
pub use service_error::*;
