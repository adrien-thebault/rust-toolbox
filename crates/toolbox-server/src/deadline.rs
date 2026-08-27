//! Request deadlines, propagated the way gRPC already does it.
//!
//! It encodes the decision that a deadline is absolute and travels with the
//! request, so a gateway timeout stops the backends working on a request nobody
//! is waiting for any more.

mod layer;

use std::time::{Duration, Instant};

use http::{HeaderName, HeaderValue};
pub use layer::{DeadlineLayer, DeadlineService};

/// The gRPC deadline header, which this reads on the way in and
/// `toolbox-grpc` writes on the way out.
pub const GRPC_TIMEOUT: HeaderName = HeaderName::from_static("grpc-timeout");

tokio::task_local! {
    /// When the request being handled must be finished.
    ///
    /// Set whenever a deadline applies. Code that fans out to a backend reads
    /// it to decide how long the call downstream may take.
    pub static DEADLINE: Instant;
}

/// How long the request being handled has left, when it has a deadline.
#[must_use]
pub fn time_remaining() -> Option<Duration> {
    DEADLINE
        .try_with(|d| d.saturating_duration_since(Instant::now()))
        .ok()
}

/// The deadline of the request being handled, when it has one.
#[must_use]
pub fn current_deadline() -> Option<Instant> {
    DEADLINE.try_with(|d| *d).ok()
}

/// Parse a gRPC `grpc-timeout` value: a positive integer and a unit character.
///
/// `H`ours, `M`inutes, `S`econds, `m`illiseconds, `u`microseconds,
/// `n`anoseconds - the units the gRPC wire specification defines.
///
/// # Arguments
///
/// * `value` - A `grpc-timeout` header value: digits then one unit character.
///   Anything else is `None`, which means no deadline rather than a wrong one.
#[must_use]
pub fn parse_grpc_timeout(value: &str) -> Option<Duration> {
    let value = value.trim();
    let (digits, unit) = value.split_at(value.len().checked_sub(1)?);
    let n: u64 = digits.parse().ok()?;
    match unit {
        "H" => Some(Duration::from_secs(n.checked_mul(3600)?)),
        "M" => Some(Duration::from_secs(n.checked_mul(60)?)),
        "S" => Some(Duration::from_secs(n)),
        "m" => Some(Duration::from_millis(n)),
        "u" => Some(Duration::from_micros(n)),
        "n" => Some(Duration::from_nanos(n)),
        _ => None,
    }
}

/// Render a `Duration` in the gRPC timeout format, always in milliseconds.
///
/// # Arguments
///
/// * `d` - The remaining budget. Always rendered in milliseconds, so successive
///   hops cannot compound a rounding error.
#[must_use]
pub fn format_grpc_timeout(d: Duration) -> HeaderValue {
    let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
    HeaderValue::from_str(&format!("{ms}m")).unwrap_or_else(|_| HeaderValue::from_static("0m"))
}
