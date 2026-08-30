//! Per-IP request throttling.
//!
//! It fixes the key `tower_governor` extracts and the shape of the response it
//! returns, so a throttled request is not the one response in the API that does
//! not look like every other error.
//!
//! **`tower_governor` is kept, and so is the custom key extractor.** Not
//! because the upstream extractor is broken - behind a proxy that replaces
//! `X-Forwarded-For` it is usually correct - but because this crate needs a
//! `client_ip` function anyway, for access logs, audit records and analytics.
//! Once it exists, using it here guarantees that "the client IP" means the
//! same thing in all four.
//!
//! **This limiter is per-process.** Three replicas means three times the
//! intended allowance. That is degradation rather than breakage, so
//! [`RateLimitAdapter`] declares `LocalDegraded` and the startup guard warns
//! rather than refusing.

use std::{net::IpAddr, time::Duration};

use axum::response::{IntoResponse, Response};
use governor::{clock::QuantaInstant, middleware::RateLimitingMiddleware};
use toolbox_cluster::deployment::{Adapter, Scope};
use tower_governor::{GovernorError, governor::GovernorConfig, key_extractor::KeyExtractor};

use crate::{
    client_ip::{TrustedHops, bucket, client_ip_of},
    error::ApiError,
};

/// Keys a limiter by the same client IP the rest of the toolbox uses.
///
/// IPv6 addresses are bucketed by their /64 prefix: an attacker holding a /64
/// otherwise has 2^64 distinct keys to spend, and a keyed limiter grows one
/// entry per key.
#[derive(Debug, Clone, Copy, Default)]
pub struct ForwardedForKeyExtractor {
    /// How many proxies to trust when picking the client entry.
    hops: TrustedHops,
}

impl ForwardedForKeyExtractor {
    /// An extractor trusting `hops` proxies. See [`TrustedHops`].
    ///
    /// # Arguments
    ///
    /// * `hops` - How many proxies append to `X-Forwarded-For`. It must match
    ///   what the rest of the process uses, or the limiter keys on a different
    ///   caller than the logs do.
    #[must_use]
    pub fn new(hops: TrustedHops) -> Self {
        Self { hops }
    }
}

impl KeyExtractor for ForwardedForKeyExtractor {
    type Key = IpAddr;

    #[cfg(feature = "governor-tracing")]
    fn name(&self) -> &'static str {
        "forwarded-for"
    }

    fn extract<T>(&self, req: &http::Request<T>) -> Result<Self::Key, GovernorError> {
        client_ip_of(req.headers(), req.extensions(), self.hops)
            .map(bucket)
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

/// Turn a limiter rejection into the same problem document as every other
/// error, with `Retry-After` and the IETF `RateLimit` fields.
///
/// A naive limiter computed the wait and then discarded it, so a client
/// had no way to know when to try again except by guessing.
///
/// # Arguments
///
/// * `err` - The rejection, which carries the wait the limiter computed. That
///   number becomes `Retry-After` instead of being discarded.
#[must_use]
pub fn error_response_handler(err: GovernorError) -> Response {
    match err {
        GovernorError::TooManyRequests { wait_time, .. } => {
            let mut response = ApiError::of_kind(
                toolbox_core::ErrorKind::ResourceExhausted,
                "Too Many Requests",
            )
            .with_code("RATE_LIMITED")
            .with_detail("too many requests; slow down")
            .with_retry_after(wait_time)
            .into_response();

            // The IETF draft field names, so a client that already understands
            // them needs no special case.
            if let Ok(v) = http::HeaderValue::from_str(&wait_time.to_string()) {
                response.headers_mut().insert("ratelimit-reset", v);
            }
            response
                .headers_mut()
                .insert("ratelimit-remaining", http::HeaderValue::from_static("0"));
            response
        }
        GovernorError::UnableToExtractKey => {
            ApiError::of_kind(toolbox_core::ErrorKind::InvalidArgument, "Invalid Argument")
                .with_code("UNIDENTIFIED_CLIENT")
                .with_detail("the client address could not be determined")
                .into_response()
        }
        GovernorError::Other { code, msg, .. } => ApiError::new(code, "Rate Limiter Error")
            .with_code("RATE_LIMITER_ERROR")
            .with_detail(msg.unwrap_or_default())
            .into_response(),
    }
}

/// Declares the limiter to the deployment guard.
#[derive(Debug, Clone, Copy, Default)]
pub struct RateLimitAdapter;

impl Adapter for RateLimitAdapter {
    fn name(&self) -> &'static str {
        "tower_governor"
    }

    fn scope(&self) -> Scope {
        Scope::LocalDegraded {
            note: "rate limiting is per-process, so N replicas enforce N times the allowance",
        }
    }

    fn remedy(&self) -> Option<&'static str> {
        Some("acceptable for login throttling; not for quota enforcement")
    }
}

/// Evict keys nobody has used lately, forever.
///
/// A keyed limiter grows one entry per distinct key and never shrinks on its
/// own, so even with correct extraction and /64 bucketing the state store is a
/// slow memory leak. Spawn this once next to the limiter it cleans.
///
/// # Arguments
///
/// * `config` - The limiter whose key space to prune. Its state grows one entry
///   per distinct key and never shrinks on its own.
/// * `every` - How often to sweep. It trades memory held against the work of
///   walking the key space.
pub fn spawn_key_space_cleanup<K, M>(
    config: &GovernorConfig<K, M>,
    every: Duration,
) -> tokio::task::JoinHandle<()>
where
    K: KeyExtractor + Send + Sync + 'static,
    K::Key: Send + Sync + 'static,
    M: RateLimitingMiddleware<QuantaInstant> + Send + Sync + 'static,
{
    let limiter = config.limiter().clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(every);
        loop {
            ticker.tick().await;
            limiter.retain_recent();
        }
    })
}
