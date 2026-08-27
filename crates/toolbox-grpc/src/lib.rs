//! tonic building blocks.
//!
//! One `ErrorInfo` across the gRPC boundary, so a gateway can rebuild the
//! problem document the originating service would have produced.
//!
//! # `toolbox-web` does not depend on this crate
//!
//! The `Status -> ErrorInfo` direction lives here, `ErrorInfo -> ApiError`
//! lives in `toolbox-web`, and a gateway composes the two. That is what lets a
//! plain HTTP project avoid compiling tonic, which a naive `axum`
//! feature made impossible.

pub mod auth;
pub mod backend;
pub mod discovery;
pub mod error;
pub mod pagination;
pub mod proto;
pub mod retry;
pub mod serve;
pub mod status;

pub use auth::{SERVICE_AUTH_HEADER, ServiceAuth, ServiceAuthLayer, require_service_auth};
pub use backend::{
    BackendChannel, BackendConfig, BackendInterceptor, BackendService, MessageLimits, backend,
};
pub use discovery::Discovery;
pub use error::GrpcError;
pub use pagination::{PROTO_INCLUDE, PageInfo, PageRequestProto, split};
pub use retry::{Backoff, RetryPolicy, is_retryable, with_retry};
pub use serve::{GrpcConfig, GrpcServerBuilder, serve_grpc};
pub use status::{GrpcResult, code_for, from_status, kind_for, to_status};
