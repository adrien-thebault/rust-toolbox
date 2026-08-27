//! The transport-neutral error vocabulary.
//!
//! One error description that both `tonic::Status` and an HTTP problem document
//! can be built from, so a domain error is classified once rather than once per
//! transport.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What kind of failure occurred, independent of transport.
///
/// `toolbox-grpc` maps this to a `tonic::Code` and `toolbox-web` to an HTTP
/// status; neither mapping lives here, which is what keeps this crate free of
/// both dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The addressed resource does not exist.
    NotFound,
    /// The request was malformed or failed validation.
    InvalidArgument,
    /// No credentials were supplied, or they were not valid.
    Unauthenticated,
    /// Credentials were valid but do not grant this operation.
    PermissionDenied,
    /// The request conflicts with current state: a duplicate key, or a failed
    /// optimistic-locking check.
    Conflict,
    /// A quota or rate limit was exhausted.
    ResourceExhausted,
    /// The request was well-formed but the resource state forbids it.
    FailedPrecondition,
    /// An unexpected failure. Its detail is never shown to a caller.
    Internal,
    /// The operation is not implemented.
    Unimplemented,
    /// A dependency is temporarily unavailable.
    Unavailable,
    /// The deadline passed before the operation completed.
    Timeout,
}

impl ErrorKind {
    /// Whether a caller can retry the same request and expect a different
    /// outcome.
    #[must_use]
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Unavailable | Self::Timeout | Self::ResourceExhausted
        )
    }
}

/// A description of an error, mirroring `google.rpc.ErrorInfo`.
///
/// This is not itself an error: it is the machine-readable part that survives
/// both the gRPC and the HTTP boundary unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// A stable, `SCREAMING_SNAKE_CASE` identifier for the failure.
    pub code: String,
    /// The service that produced it, so codes from different services cannot
    /// collide.
    pub domain: String,
    /// Structured context. A `BTreeMap` rather than a `HashMap` so the
    /// serialized form is deterministic and diffable.
    pub metadata: BTreeMap<String, String>,
}

impl ErrorInfo {
    /// Build an `ErrorInfo` with no metadata.
    ///
    /// # Arguments
    ///
    /// * `code` - A stable, `SCREAMING_SNAKE_CASE` identifier for the failure.
    ///   It is what a client branches on, so it must not change once released.
    /// * `domain` - The service that produced the failure, so codes from two
    ///   services cannot collide.
    pub fn new(code: impl Into<String>, domain: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            domain: domain.into(),
            metadata: BTreeMap::new(),
        }
    }

    /// Attach one metadata entry.
    ///
    /// # Arguments
    ///
    /// * `key` - The metadata name. Keys are sorted in the serialized form, so
    ///   this also fixes the output order.
    /// * `value` - The metadata value. It crosses the wire to the client, so it
    ///   must not carry anything internal.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// A domain error that can describe itself to any transport.
///
/// Implement this on your service's error enum; `toolbox-grpc::to_status` and
/// `toolbox-web::ApiError` both consume it. There is deliberately no blanket
/// `impl<E: ServiceError> From<E> for tonic::Status` - see `toolbox-grpc`.
pub trait ServiceError: std::error::Error {
    /// A stable `SCREAMING_SNAKE_CASE` code. Clients match on this, never on
    /// the `Display` string.
    fn code(&self) -> &'static str;

    /// The owning service, so codes cannot collide across services.
    fn domain(&self) -> &'static str;

    /// How the transport should classify this error.
    fn kind(&self) -> ErrorKind;

    /// Structured context for the caller. Never put anything here that a 5xx
    /// must not leak: `ApiError` redacts `detail` on 5xx but this map is the
    /// caller's contract.
    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    /// The machine-readable description, built from the three required methods.
    fn info(&self) -> ErrorInfo {
        ErrorInfo {
            code: self.code().to_owned(),
            domain: self.domain().to_owned(),
            metadata: self.metadata(),
        }
    }
}
