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

use std::{fmt, sync::Arc, time::Duration};

use axum::response::{IntoResponse, Response};
use http::{StatusCode, header::CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use toolbox_cluster::{KvStore, KvStoreError};
use toolbox_error::ErrorKind;
use tracing::warn;

use crate::{error::ApiError, extract::IdempotencyKey};

/// How long a stored response is replayable.
pub const DEFAULT_TTL: Duration = Duration::from_hours(24);

/// How long a handler may own a claim before a retry can take it over.
pub const DEFAULT_CLAIM_TTL: Duration = Duration::from_mins(5);

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

impl StoredResponse {
    /// A `200 application/json` record of `value`.
    ///
    /// The record that [`Idempotent::json`](crate::extract::Idempotent::json)
    /// stores, and what a handler holding the raw [`Idempotency`] primitives
    /// builds to pass to [`Idempotency::record`].
    ///
    /// # Errors
    /// [`ApiError`] when `value` cannot be serialised.
    pub fn json<T: Serialize>(value: &T) -> Result<Self, ApiError> {
        Ok(Self {
            status: StatusCode::OK.as_u16(),
            body: serde_json::to_vec(value).map_err(ApiError::internal)?,
            content_type: "application/json".to_owned(),
        })
    }

    /// Rebuild this record as an HTTP response, for a replay.
    #[must_use]
    pub fn replay(&self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK);
        (
            status,
            [(CONTENT_TYPE, self.content_type.clone())],
            self.body.clone(),
        )
            .into_response()
    }
}

/// What a claim attempt found.
#[derive(Debug)]
pub enum IdempotencyOutcome {
    /// This caller owns the key and should run the handler.
    Fresh(IdempotencyClaim),
    /// The first request finished; replay its response.
    Replay(Box<StoredResponse>),
    /// The first request is still running.
    InFlight,
}

/// Proof that this handler owns an in-flight key.
///
/// It is deliberately opaque. Passing the claim to [`Idempotency::record`] or
/// [`Idempotency::release`] makes those operations conditional on the same
/// owner still holding the key.
#[derive(Debug)]
pub struct IdempotencyClaim {
    /// The fully scoped store key.
    key: String,
    /// A random per-attempt marker stored as the value.
    marker: Vec<u8>,
}

/// Prefix distinguishing an owner marker from a JSON response.
const IN_FLIGHT_PREFIX: &[u8] = b"\x00in-flight:";

/// Claims keys and stores responses against them.
pub struct Idempotency {
    /// Where in-flight markers and stored responses live.
    kv: Arc<dyn KvStore>,
    /// How long a stored response is replayable.
    ttl: Duration,
    /// How long a handler owns an unfinished claim.
    claim_ttl: Duration,
}

impl fmt::Debug for Idempotency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Idempotency")
            .field("ttl", &self.ttl)
            .field("claim_ttl", &self.claim_ttl)
            .finish_non_exhaustive()
    }
}

impl Idempotency {
    /// Build over a key-value store.
    ///
    /// # Arguments
    ///
    /// * `kv` - The store. `KvStore::add` is atomic by contract, which is what
    ///   stops two racing retries both claiming the same key.
    #[must_use]
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self {
            kv,
            ttl: DEFAULT_TTL,
            claim_ttl: DEFAULT_CLAIM_TTL,
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

    /// How long an unfinished handler owns its claim.
    ///
    /// The response TTL and claim TTL are separate: successful results usually
    /// remain replayable much longer than a handler should be allowed to run.
    #[must_use]
    pub fn claim_ttl(mut self, claim_ttl: Duration) -> Self {
        self.claim_ttl = claim_ttl;
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
    pub async fn claim(
        &self,
        key: &IdempotencyKey,
        route: &str,
    ) -> Result<IdempotencyOutcome, ApiError> {
        // Scoped by route as well as key, because two endpoints given the same
        // client-chosen key are two different operations - and replaying one's
        // response for the other would be worse than not replaying at all.
        let key = storage_key(route, key);
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(ApiError::internal)?;
        let mut marker = Vec::with_capacity(IN_FLIGHT_PREFIX.len() + random.len());
        marker.extend_from_slice(IN_FLIGHT_PREFIX);
        marker.extend_from_slice(&random);

        // One retry covers an entry expiring between the failed `add` and the
        // following `get`. Seeing that race twice means the store cannot give
        // this operation a stable view, so fail rather than spin forever.
        for attempt in 0..=1 {
            // Atomic create: two racing first requests cannot both win the
            // claim, because exactly one `add` returns `true`.
            if self
                .kv
                .add(&key, marker.clone(), Some(self.claim_ttl))
                .await
                .map_err(store_error)?
            {
                return Ok(IdempotencyOutcome::Fresh(IdempotencyClaim { key, marker }));
            }

            match self.kv.get(&key).await.map_err(store_error)? {
                // The entry expired between `add` and `get`. Retry the atomic
                // add; merely returning Fresh here would run without owning it.
                None if attempt == 0 => {}
                None => return Err(store_unavailable_error()),
                Some(raw) if raw.starts_with(IN_FLIGHT_PREFIX) => {
                    return Ok(IdempotencyOutcome::InFlight);
                }
                Some(raw) => match serde_json::from_slice(&raw) {
                    Ok(stored) => return Ok(IdempotencyOutcome::Replay(Box::new(stored))),
                    Err(e) => {
                        warn!(error = %e, "an idempotency record could not be decoded");
                        return Err(corrupt_record_error());
                    }
                },
            }
        }
        Err(store_unavailable_error())
    }

    /// Record the response for a key.
    ///
    /// # Arguments
    ///
    /// * `claim` - The ownership proof returned by [`Idempotency::claim`].
    /// * `response` - The status, headers and body to replay on a repeat.
    ///
    /// # Errors
    /// [`ApiError`] when the store fails.
    pub async fn record(
        &self,
        claim: IdempotencyClaim,
        response: &StoredResponse,
    ) -> Result<(), ApiError> {
        let value = serde_json::to_vec(response).map_err(ApiError::internal)?;
        let stored = self
            .kv
            .replace_if_matches(&claim.key, &claim.marker, Some(value), Some(self.ttl))
            .await
            .map_err(store_error)?;

        stored.ok_or_else(claim_lost_error)
    }

    /// Release a claim without recording a response.
    ///
    /// Called when the handler failed: a 5xx is not an outcome worth replaying,
    /// and leaving the key claimed would make the retry - the entire point of
    /// sending a key - impossible.
    ///
    /// # Arguments
    ///
    /// * `claim` - The ownership proof returned by [`Idempotency::claim`].
    ///
    /// # Errors
    /// [`ApiError`] when the store fails.
    pub async fn release(&self, claim: IdempotencyClaim) -> Result<(), ApiError> {
        self.kv
            .replace_if_matches(&claim.key, &claim.marker, None, None)
            .await
            .map(|_| ())
            .map_err(store_error)
    }
}

/// The error a claimed-but-unfinished key produces.
#[must_use]
pub fn in_flight_error() -> ApiError {
    ApiError::of_kind(ErrorKind::Conflict, "Conflict")
        .with_code("IDEMPOTENCY_IN_FLIGHT")
        .with_detail("a request with this Idempotency-Key is still being processed")
}

/// A completed request can no longer publish after its ownership expired.
fn claim_lost_error() -> ApiError {
    ApiError::of_kind(ErrorKind::Conflict, "Conflict")
        .with_code("IDEMPOTENCY_CLAIM_LOST")
        .with_detail("the idempotency claim expired before the response was recorded")
}

/// Corrupt shared state is not permission to execute a potentially duplicate
/// side effect.
fn corrupt_record_error() -> ApiError {
    ApiError::of_kind(ErrorKind::Unavailable, "Service Unavailable")
        .with_code("IDEMPOTENCY_RECORD_INVALID")
}

/// An unstable store cannot safely decide whether running the handler would
/// duplicate an operation.
fn store_unavailable_error() -> ApiError {
    ApiError::of_kind(ErrorKind::Unavailable, "Service Unavailable")
        .with_code("IDEMPOTENCY_STORE_UNAVAILABLE")
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
fn store_error(e: KvStoreError) -> ApiError {
    store_unavailable_error().with_source(e)
}
