//! axum building blocks.
//!
//! One error shape across every route, and extractors that put a check in the
//! signature where it cannot be forgotten.
//!
//! # This crate does not depend on `toolbox-grpc`
//!
//! A plain HTTP project must never compile `tonic`. The single `ErrorInfo` in
//! `toolbox-error` is what keeps them apart: `toolbox-grpc` owns
//! `Status -> ErrorInfo`, this crate owns `ErrorInfo -> ApiError`, and a
//! gateway composes the two. There is no exception and no feature that
//! reintroduces one.

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
#[cfg(feature = "openapi")]
pub mod openapi;
pub mod pagination;
#[cfg(feature = "rate-limit")]
pub mod rate_limit;
#[cfg(feature = "realtime")]
pub mod realtime;
pub mod server;

#[cfg(feature = "auth-router")]
pub use auth::{AuthState, auth_router, session_layer};
pub use client_ip::{ClientIpTrustPolicy, client_ip, resolve_client_ip};
pub use cors::{cors, cors_localhost};
pub use error::{ApiError, status_for};
#[cfg(feature = "auth-router")]
pub use extract::QueryAuthenticated;
pub use extract::{Authenticated, Idempotent, MaybeAuthenticated, PageQuery, ValidJson};
pub use health::{HealthCheck, HealthCheckResult, HealthResponse, HealthState, health_router};
#[cfg(feature = "idempotency")]
pub use idempotency::{Idempotency, IdempotencyOutcome, StoredResponse, in_flight_error};
#[cfg(feature = "openapi")]
pub use openapi::{OpenApiConfig, openapi_router, serialize_openapi, with_standard_errors};
pub use pagination::{attach_page_headers, page_links};
#[cfg(feature = "rate-limit")]
pub use rate_limit::RateLimitConfig;
pub use server::{WebServerConfig, serve};
