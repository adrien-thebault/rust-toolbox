//! The idempotency-key extractor.
//!
//! The header name is the IETF draft's `Idempotency-Key`, not a bespoke one, so
//! a client library that already knows the convention works unchanged.
//!
//! This reads and validates the key. Replaying the first response for a
//! repeated key is [`Idempotent::json`] - gated on the `idempotency` feature,
//! since it needs the `crate::idempotency::Idempotency` store.

#[cfg(feature = "idempotency")]
use std::future::Future;

use axum::extract::FromRequestParts;
#[cfg(feature = "idempotency")]
use axum::response::Response;
use http::{HeaderName, request::Parts};
#[cfg(feature = "idempotency")]
use serde::Serialize;

use crate::error::ApiError;
#[cfg(feature = "idempotency")]
use crate::idempotency::{Idempotency, IdempotencyOutcome, StoredResponse, in_flight_error};

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

impl Idempotent {
    /// Run `handler` under this request's `Idempotency-Key`, replaying the
    /// first response for a repeat and returning a `200 application/json`
    /// response either way.
    ///
    /// The whole `claim` -> run -> `record`/`release` dance, which was ~20
    /// lines copied into every keyed route. A request with no key runs
    /// `handler` straight through, unchanged - sending a key is what opts in.
    ///
    /// A failed run releases the claim rather than recording it: a 5xx is not
    /// an outcome worth replaying, and leaving the key claimed would make the
    /// retry the key exists for impossible.
    ///
    /// Gated on the `idempotency` feature, which brings the
    /// `crate::idempotency::Idempotency` store it needs.
    ///
    /// ```ignore
    /// pub async fn upload(
    ///     State(state): State<AppState>,
    ///     idempotent: Idempotent,
    ///     Json(body): Json<UploadBody>,
    /// ) -> Result<Response, ApiError> {
    ///     idempotent
    ///         .json(&state.idempotency, "upload", || do_upload(&state, body))
    ///         .await
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `store` - The claim/replay store, from the app state.
    /// * `route` - A stable label scoping the key, so the same client key on
    ///   two endpoints cannot replay the wrong response. Usually the handler
    ///   name.
    /// * `handler` - Produces the value to serialise, or an [`ApiError`]. Runs
    ///   at most once.
    ///
    /// # Errors
    /// [`ApiError`]: whatever `handler` returns, a store failure, or `409`
    /// when a first request with the same key is still in flight.
    #[cfg(feature = "idempotency")]
    pub async fn json<T, F, Fut>(
        self,
        store: &Idempotency,
        route: &str,
        handler: F,
    ) -> Result<Response, ApiError>
    where
        T: Serialize,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let Some(key) = self.0 else {
            return Ok(StoredResponse::json(&handler().await?)?.replay());
        };

        match store.claim(&key, route).await? {
            IdempotencyOutcome::InFlight => Err(in_flight_error()),
            IdempotencyOutcome::Replay(stored) => Ok(stored.replay()),
            IdempotencyOutcome::Fresh => match handler().await {
                Ok(value) => {
                    let stored = StoredResponse::json(&value)?;
                    store.record(&key, route, &stored).await?;
                    Ok(stored.replay())
                }
                Err(e) => {
                    store.release(&key, route).await?;
                    Err(e)
                }
            },
        }
    }
}
