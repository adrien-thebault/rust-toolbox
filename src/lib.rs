//! generic, project-agnostic building blocks (diesel repositories, tower
//! layers, axum auth/errors, a tonic `ServiceError` type, an SMTP sender)
//! shared by every backend crate. Each module is behind its own feature
//! flag - see this crate's `Cargo.toml`.
#![warn(missing_docs)]

/// diesel traits and types
#[cfg(feature = "diesel")]
pub mod diesel_tools;

/// tower traits and types
#[cfg(feature = "tower")]
pub mod tower_tools;

/// ServiceError trait + tonic::Status <-> google.rpc.ErrorInfo conversion
#[cfg(feature = "tonic")]
pub mod tonic_tools;

/// generic axum-based web-gateway building blocks: auth/RBAC, HTTP errors
#[cfg(feature = "axum")]
pub mod axum_tools;

/// a reusable SMTP sender, configured explicitly rather than from the environment
#[cfg(feature = "mail")]
pub mod mail_tools;
