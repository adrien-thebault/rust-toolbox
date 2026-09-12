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
//! intended allowance - acceptable for login throttling, not for quota
//! enforcement.

use std::{net::IpAddr, sync::Arc, time::Duration};

use axum::response::{IntoResponse, Response};
use governor::{
    clock::QuantaInstant,
    middleware::{NoOpMiddleware, RateLimitingMiddleware},
};
use tower_governor::{
    GovernorError, GovernorLayer,
    governor::{GovernorConfig, GovernorConfigBuilder},
    key_extractor::KeyExtractor,
};

use crate::{
    client_ip::{ClientIpTrustPolicy, bucket, client_ip_of},
    error::ApiError,
};

/// Keys a limiter by the same client IP the rest of the toolbox uses.
///
/// IPv6 addresses are bucketed by their /64 prefix: an attacker holding a /64
/// otherwise has 2^64 distinct keys to spend, and a keyed limiter grows one
/// entry per key.
#[derive(Debug, Clone)]
pub struct ForwardedForKeyExtractor {
    /// How the client entry is picked out of `X-Forwarded-For`.
    trust: ClientIpTrustPolicy,
}

impl ForwardedForKeyExtractor {
    /// An extractor using `trust` to find the client. See [`ClientIpTrustPolicy`].
    ///
    /// # Arguments
    ///
    /// * `trust` - How to read `X-Forwarded-For`. It must match what the rest
    ///   of the process uses, or the limiter keys on a different caller than
    ///   the logs do.
    #[must_use]
    pub fn new(trust: ClientIpTrustPolicy) -> Self {
        Self { trust }
    }
}

impl KeyExtractor for ForwardedForKeyExtractor {
    type Key = IpAddr;

    #[cfg(feature = "governor-tracing")]
    fn name(&self) -> &'static str {
        "forwarded-for"
    }

    fn extract<T>(&self, req: &http::Request<T>) -> Result<Self::Key, GovernorError> {
        client_ip_of(req.headers(), req.extensions(), &self.trust)
            .map(bucket)
            .ok_or(GovernorError::UnableToExtractKey)
    }
}

/// A per-IP throttle: `burst` requests, then one back every `replenish_every`.
///
/// The values have no safe default - a login endpoint wants a few per minute, a
/// public read endpoint wants far more - so all three are stated. `trust`
/// decides how the caller is identified; it must match the rest of the process.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// How many requests one caller may make back to back.
    pub burst: u32,
    /// How long before one spent request is given back.
    pub replenish_every: Duration,
    /// How the caller is identified behind proxies.
    pub trust: ClientIpTrustPolicy,
}

impl RateLimitConfig {
    /// A throttle allowing `burst` requests, replenished one per
    /// `replenish_every`, keyed by `trust`.
    #[must_use]
    pub fn new(burst: u32, replenish_every: Duration, trust: ClientIpTrustPolicy) -> Self {
        Self {
            burst,
            replenish_every,
            trust,
        }
    }

    /// The tower layer. It keys on [`ForwardedForKeyExtractor`] and answers a
    /// rejection through [`error_response_handler`], so a throttled request
    /// looks like every other error.
    ///
    /// `burst` and the period are clamped above zero here, which is the only
    /// way `finish` returns `None`, so the `expect` is unreachable - no
    /// `# Panics`.
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn layer(
        &self,
    ) -> GovernorLayer<ForwardedForKeyExtractor, NoOpMiddleware<QuantaInstant>, axum::body::Body>
    {
        let config = GovernorConfigBuilder::default()
            .key_extractor(ForwardedForKeyExtractor::new(self.trust.clone()))
            .per_millisecond(
                u64::try_from(self.replenish_every.as_millis())
                    .unwrap_or(u64::MAX)
                    .max(1),
            )
            .burst_size(self.burst.max(1))
            .finish()
            .expect("a non-zero burst and period");

        GovernorLayer::new(Arc::new(config)).error_handler(error_response_handler)
    }
}

/// Turn a limiter rejection into the same problem document as every other
/// error, with `Retry-After` and the IETF `RateLimit` fields.
///
/// The wait the limiter computes is carried through to the client so it knows
/// when to try again rather than guessing.
///
/// # Arguments
///
/// * `err` - The rejection, which carries the wait the limiter computed. That
///   number becomes `Retry-After`.
#[must_use]
pub fn error_response_handler(err: GovernorError) -> Response {
    match err {
        GovernorError::TooManyRequests { wait_time, .. } => {
            let mut response = ApiError::of_kind(
                toolbox_error::ErrorKind::ResourceExhausted,
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
        GovernorError::UnableToExtractKey => ApiError::of_kind(
            toolbox_error::ErrorKind::InvalidArgument,
            "Invalid Argument",
        )
        .with_code("UNIDENTIFIED_CLIENT")
        .with_detail("the client address could not be determined")
        .into_response(),
        GovernorError::Other { code, msg, .. } => ApiError::new(code, "Rate Limiter Error")
            .with_code("RATE_LIMITER_ERROR")
            .with_detail(msg.unwrap_or_default())
            .into_response(),
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
