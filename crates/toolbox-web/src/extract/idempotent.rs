//! The idempotency-key extractor.
//!
//! The header name is the IETF draft's `Idempotency-Key`, not a bespoke one, so
//! a client library that already knows the convention works unchanged.
//!
//! This reads and validates the key. Replaying a stored response for a
//! repeated key needs somewhere to store it, which arrives with the
//! `KvStore` adapters in stage 5.

use axum::extract::FromRequestParts;
use http::{HeaderName, request::Parts};

use crate::error::ApiError;

/// The IETF draft header name.
pub const IDEMPOTENCY_KEY: HeaderName = HeaderName::from_static("idempotency-key");

/// The longest key accepted, so the header cannot be used as free storage.
pub const MAX_KEY_LEN: usize = 255;

/// The longest key accepted, as a function so a test cannot drift from it.
#[must_use]
pub fn idempotency_key_max_len() -> usize {
    MAX_KEY_LEN
}

/// A caller-supplied key identifying a retryable operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// The key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `Idempotency-Key` header, if the caller sent one.
#[derive(Debug, Clone)]
pub struct Idempotent(pub Option<IdempotencyKey>);

impl<S: Send + Sync> FromRequestParts<S> for Idempotent {
    type Rejection = ApiError;

    #[allow(clippy::unused_async_trait_impl)] // trait-required async signature
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(value) = parts.headers.get(IDEMPOTENCY_KEY) else {
            return Ok(Self(None));
        };

        let key = value.to_str().map_err(|_| {
            ApiError::bad_request("Idempotency-Key must be printable ASCII")
                .with_code("INVALID_IDEMPOTENCY_KEY")
        })?;

        if key.is_empty() || key.len() > MAX_KEY_LEN {
            return Err(ApiError::bad_request(format!(
                "Idempotency-Key must be 1 to {MAX_KEY_LEN} characters"
            ))
            .with_code("INVALID_IDEMPOTENCY_KEY"));
        }

        Ok(Self(Some(IdempotencyKey(key.to_owned()))))
    }
}
