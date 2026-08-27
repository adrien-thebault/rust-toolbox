//! Replaying the response for a repeated `Idempotency-Key`.
//!
//! The header name is the IETF draft's, so a client library that already knows
//! the convention needs no special case, and it removes the trap that a
//! retried `POST` charges the card twice.
//!
//! # What "in flight" means here
//!
//! A key is claimed before the handler runs. A second request with the same
//! key while the first is still running gets **409**, not a duplicate and not
//! a wait: the correct answer to "did my first request succeed?" is not
//! "here, have another one".

use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use toolbox_cluster::KeyValueStore;
use tracing::warn;

use crate::{error::ApiError, extract::IdempotencyKey};

/// How long a stored response is replayable.
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The key prefix, so idempotency records cannot collide with anything else in
/// a shared store.
const PREFIX: &str = "toolbox:idem:";

/// A recorded response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredResponse {
    /// The status the first request returned.
    pub status: u16,
    /// The body it returned.
    pub body: Vec<u8>,
    /// The content type it returned.
    pub content_type: String,
}

/// What a claim attempt found.
#[derive(Debug)]
pub enum Claim {
    /// This caller owns the key and should run the handler.
    Fresh,
    /// The first request finished; replay its response.
    Replay(Box<StoredResponse>),
    /// The first request is still running.
    InFlight,
}

/// The marker stored while a request is running.
const IN_FLIGHT: &[u8] = b"\x00in-flight";

/// Claims keys and stores responses against them.
pub struct Idempotency {
    kv: Arc<dyn KeyValueStore>,
    ttl: Duration,
}

impl std::fmt::Debug for Idempotency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Idempotency")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl Idempotency {
    /// Build over a key-value store.
    ///
    /// # Arguments
    ///
    /// * `kv` - The store. It must promise an atomic take, or two concurrent
    ///   retries could both claim the same key.
    #[must_use]
    pub fn new(kv: Arc<dyn KeyValueStore>) -> Self {
        Self {
            kv,
            ttl: DEFAULT_TTL,
        }
    }

    /// How long a response stays replayable.
    ///
    /// # Arguments
    ///
    /// * `ttl` - How long a recorded response stays replayable. Long enough for
    ///   a client's retries, short enough that the store does not grow without
    ///   end.
    #[must_use]
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Claim a key, or find out what happened to it.
    ///
    /// # Arguments
    ///
    /// * `key` - What the caller sent in `Idempotency-Key`.
    /// * `route` - The route being claimed. It is part of the storage key, so
    ///   the same client key on a different endpoint cannot replay the wrong
    ///   response.
    ///
    /// # Errors
    /// [`ApiError`] when the store fails.
    pub async fn claim(&self, key: &IdempotencyKey, route: &str) -> Result<Claim, ApiError> {
        let key = storage_key(route, key);

        // Scoped by route as well as key, because two endpoints given the same
        // client-chosen key are two different operations - and replaying one's
        // response for the other would be worse than not replaying at all.
        match self.kv.get(&key).await.map_err(store_error)? {
            None => {
                self.kv
                    .set(&key, IN_FLIGHT.to_vec(), Some(self.ttl))
                    .await
                    .map_err(store_error)?;
                Ok(Claim::Fresh)
            }
            Some(raw) if raw == IN_FLIGHT => Ok(Claim::InFlight),
            Some(raw) => match serde_json::from_slice(&raw) {
                Ok(stored) => Ok(Claim::Replay(Box::new(stored))),
                // A record we cannot read is a record we cannot honour; run
                // the handler rather than failing the request.
                Err(e) => {
                    warn!(error = %e, "an idempotency record could not be decoded");
                    Ok(Claim::Fresh)
                }
            },
        }
    }

    /// Record the response for a key.
    ///
    /// # Arguments
    ///
    /// * `key` - The key being completed.
    /// * `route` - The route it was claimed on.
    /// * `response` - The status, headers and body to replay on a repeat.
    ///
    /// # Errors
    /// [`ApiError`] when the store fails.
    pub async fn record(
        &self,
        key: &IdempotencyKey,
        route: &str,
        response: &StoredResponse,
    ) -> Result<(), ApiError> {
        let value = serde_json::to_vec(response).map_err(ApiError::internal)?;
        self.kv
            .set(&storage_key(route, key), value, Some(self.ttl))
            .await
            .map_err(store_error)
    }

    /// Release a claim without recording a response.
    ///
    /// Called when the handler failed: a 5xx is not an outcome worth replaying,
    /// and leaving the key claimed would make the retry - the entire point of
    /// sending a key - impossible.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to unclaim.
    /// * `route` - The route it was claimed on.
    ///
    /// # Errors
    /// [`ApiError`] when the store fails.
    pub async fn release(&self, key: &IdempotencyKey, route: &str) -> Result<(), ApiError> {
        self.kv
            .delete(&storage_key(route, key))
            .await
            .map_err(store_error)
    }
}

/// The error a claimed-but-unfinished key produces.
#[must_use]
pub fn in_flight_error() -> ApiError {
    ApiError::of_kind(toolbox_core::ErrorKind::Conflict, "Conflict")
        .with_code("IDEMPOTENCY_IN_FLIGHT")
        .with_detail("a request with this Idempotency-Key is still being processed")
}

/// The store key for a claim, prefixed so idempotency records cannot collide
/// with anything else in a shared store.
///
/// # Arguments
///
/// * `route` - The route, which scopes the key.
/// * `key` - The caller's key.
fn storage_key(route: &str, key: &IdempotencyKey) -> String {
    format!("{PREFIX}{route}:{key}")
}

/// A store failure as an API error. Distinct from a conflict, because the two
/// lead a client to opposite behaviours.
///
/// # Arguments
///
/// * `e` - The failure the key-value adapter reported.
fn store_error(e: toolbox_cluster::KeyValueError) -> ApiError {
    ApiError::of_kind(toolbox_core::ErrorKind::Unavailable, "Service Unavailable")
        .with_code("IDEMPOTENCY_STORE_UNAVAILABLE")
        .with_source(e)
}
