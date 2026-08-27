//! axum building blocks.
//!
//! One error shape across every route, and extractors that put a check in the
//! signature where it cannot be forgotten.
//!
//! # This crate does not depend on `toolbox-grpc`
//!
//! A naive `axum` feature *implied* `tonic`, so a plain HTTP project with no
//! gRPC anywhere compiled tonic. The single `ErrorInfo` in `toolbox-core`
//! removes the need: `toolbox-grpc` owns `Status -> ErrorInfo`, this crate
//! owns `ErrorInfo -> ApiError`, and a gateway composes the two. There is no
//! exception to this and no feature that reintroduces one.

#[cfg(feature = "auth-router")]
pub mod auth;
#[cfg(feature = "captcha")]
pub mod captcha;
pub mod client_ip;
pub mod cors;
pub mod error;
pub mod extract;
pub mod health;
#[cfg(feature = "idempotency")]
pub mod idempotency;
pub mod links;
#[cfg(feature = "openapi")]
pub mod openapi;
#[cfg(feature = "rate-limit")]
pub mod rate_limit;
#[cfg(feature = "realtime")]
pub mod realtime;
pub mod serve;

#[cfg(feature = "auth-router")]
pub use auth::{AuthState, LoginLimit, auth_router, session_layer};
pub use client_ip::{TrustedHops, client_ip, resolve_client_ip};
pub use cors::{cors, dev_and};
pub use error::{ApiError, status_for};
pub use extract::{Authenticated, Idempotent, MaybeAuthenticated, PageQuery, ValidJson};
pub use health::{Check, Health, HealthState, ReadinessCheck, health_router};
#[cfg(feature = "idempotency")]
pub use idempotency::{Claim, Idempotency, StoredResponse, in_flight_error};
pub use links::{attach_page_headers, page_links};
#[cfg(feature = "openapi")]
pub use openapi::{DocsConfig, dump_openapi, openapi_router, with_standard_errors};
pub use serve::serve_http;
