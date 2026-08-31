//! tonic building blocks.
//!
//! One `ErrorInfo` across the gRPC boundary, so a gateway can rebuild the
//! problem document the originating service would have produced.
//!
//! # `toolbox-web` does not depend on this crate
//!
//! The `Status -> ErrorInfo` direction lives here, `ErrorInfo -> ApiError`
//! lives in `toolbox-web`, and a gateway composes the two. That is what lets a
//! plain HTTP project avoid compiling tonic, which a naive `axum` feature made
//! impossible.

pub mod client;
pub mod limits;
pub mod pagination;
pub mod proto;
pub mod server;
pub mod status;

/// The metadata key carrying the shared service secret that
/// [`server::shared_secret::shared_secret_layer`] checks. Written on every
/// outbound call by [`client::interceptor::ClientInterceptor`].
pub const X_SHARED_SECRET: &str = "x-shared-secret";
/// The metadata key carrying a base64 [`toolbox_auth::ForwardedPrincipal`],
/// resolved by [`server::identity::identity_layer`]. Written only
/// while a [`client::forwarding`] scope is active.
pub const X_FWD_PRINCIPAL: &str = "x-fwd-principal";

pub use client::{
    Backoff, ClientChannel, ClientConfig, ClientError, ClientInterceptor, ClientService,
    RetryPolicy, client, forwarding, is_retryable, with_retry,
};
pub use limits::MessageLimits;
pub use pagination::{PROTO_INCLUDE, PageInfo, PageRequestProto, split};
pub use server::{
    ServerBuilder, ServerConfig, identity, identity::identity_layer, serve,
    shared_secret::shared_secret_layer,
};
pub use status::{GrpcResult, code_for, from_status, kind_for, to_status};
