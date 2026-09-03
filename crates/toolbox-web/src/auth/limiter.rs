//! The throttle on the credential routes.

use std::{sync::Arc, time::Duration};

use governor::{clock::QuantaInstant, middleware::NoOpMiddleware};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

use crate::{
    client_ip::TrustedHops,
    rate_limit::{ForwardedForKeyExtractor, error_response_handler},
};

/// How the credential routes are throttled.
///
/// Unauthenticated endpoints that check a secret are the ones worth limiting:
/// without this, an attacker gets as many password guesses per second as the
/// argon2 cost allows.
#[derive(Debug, Clone, Copy)]
pub struct LoginLimit {
    /// How many attempts one caller may make back to back.
    pub burst: u32,
    /// How long before one of those attempts is replenished.
    pub replenish_every: Duration,
    /// How many proxies append to `X-Forwarded-For`.
    ///
    /// It must match what the rest of the process uses. Set too low behind a
    /// proxy, the limiter keys on the proxy's address and the first attacker
    /// locks out every other caller.
    pub hops: TrustedHops,
}

impl Default for LoginLimit {
    /// Five attempts, then one every five seconds.
    ///
    /// Enough that a person mistyping a password three times notices nothing,
    /// slow enough that credential stuffing from one address is pointless.
    fn default() -> Self {
        Self {
            burst: 5,
            replenish_every: Duration::from_secs(5),
            hops: TrustedHops::default(),
        }
    }
}

/// The throttle applied to the credential routes.
///
/// Private, so the `expect` below is not a documented panic: `burst` and the
/// period are both clamped above zero here, which is the only way `finish`
/// returns `None`.
///
/// # Arguments
///
/// * `limit` - What to build the limiter from.
pub(super) fn login_limiter(
    limit: &LoginLimit,
) -> GovernorLayer<ForwardedForKeyExtractor, NoOpMiddleware<QuantaInstant>, axum::body::Body> {
    let config = GovernorConfigBuilder::default()
        .key_extractor(ForwardedForKeyExtractor::new(limit.hops))
        .per_millisecond(
            u64::try_from(limit.replenish_every.as_millis())
                .unwrap_or(u64::MAX)
                .max(1),
        )
        .burst_size(limit.burst.max(1))
        .finish()
        .expect("a non-zero burst and period");

    GovernorLayer::new(Arc::new(config)).error_handler(error_response_handler)
}
