//! generic, domain-agnostic building blocks for an axum-based HTTP gateway
//! sitting in front of one or more gRPC backends: authentication/RBAC and an
//! HTTP error type aware of `tonic::Status`. Nothing here knows about any
//! particular gateway's routes or backends - see the `web` crate for that.

pub mod api_error;
pub mod auth;
pub mod controller;
