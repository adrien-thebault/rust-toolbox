//! One correctly-ordered layer stack per transport.
//!
//! Layer order is a decision that is re-made, and got wrong, in every project -
//! axum composes outward from the router while tonic composes outward from the
//! server, so the same `ServiceBuilder` is the only way both read alike.
//! Composing inside a single builder means `Router::layer` and `Server::layer`
//! behave identically.
//!
//! One file per transport, each holding its stack, the service type it
//! produces and the function that builds it. [`StackConfig`] is what they
//! share, and the three files sitting side by side is what makes a difference
//! between two stacks a diff rather than a hunt.

mod grpc;
mod http;
mod realtime;

use std::time::Duration;

pub use grpc::{GrpcStack, GrpcStacked, grpc_stack};
pub use http::{HttpStack, HttpStacked, http_stack};
pub use realtime::{RealtimeStack, RealtimeStacked, realtime_stack};
use tracing::Level;

/// What the standard stacks do, and what they refuse to do by default.
#[derive(Debug, Clone, Copy)]
pub struct StackConfig {
    /// How long a request may take before it is answered with a timeout.
    ///
    /// A caller's own `grpc-timeout` may shorten this but never extend it.
    pub timeout: Option<Duration>,
    /// The largest request body accepted.
    ///
    /// Applied by the transport crate rather than by these stacks, because
    /// each ecosystem already has the right mechanism and neither can be
    /// expressed generically: `toolbox-web` turns this into axum's
    /// `DefaultBodyLimit` and `toolbox-grpc` into tonic's
    /// `max_decoding_message_size`. `tower_http::limit` cannot be used here -
    /// it rewrites the request body type to `Limited<B>`, which every router
    /// below the stack would then have to accept.
    ///
    /// The default is small on purpose. An upload route must be layered
    /// separately with its own limit - `files_router` does that from the
    /// `UploadPolicy` it already has - because a global limit large enough for
    /// uploads is a limit that protects nothing.
    pub max_body_bytes: Option<usize>,
    /// The level request spans are recorded at.
    pub trace_level: Level,
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(30)),
            max_body_bytes: Some(2 * 1024 * 1024),
            trace_level: Level::INFO,
        }
    }
}

impl StackConfig {
    /// No timeout and no body limit, for a stack serving long-lived streams.
    #[must_use]
    pub fn unbounded() -> Self {
        Self {
            timeout: None,
            max_body_bytes: None,
            trace_level: Level::INFO,
        }
    }

    /// Set the request timeout.
    ///
    /// # Arguments
    ///
    /// * `timeout` - The ceiling for a request that carries no deadline of its
    ///   own. `None` disables it, which is what the realtime stack needs.
    #[must_use]
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the request body limit.
    ///
    /// # Arguments
    ///
    /// * `max` - The largest request body accepted. `None` leaves it to axum's
    ///   `DefaultBodyLimit` or tonic's decoding limit, which is where it
    ///   belongs.
    #[must_use]
    pub fn max_body_bytes(mut self, max: Option<usize>) -> Self {
        self.max_body_bytes = max;
        self
    }

    /// Set the level request spans are recorded at.
    ///
    /// # Arguments
    ///
    /// * `level` - What level request spans are recorded at.
    #[must_use]
    pub fn trace_level(mut self, level: Level) -> Self {
        self.trace_level = level;
        self
    }

    /// The body limit in bytes, with `None` meaning unlimited.
    #[must_use]
    pub fn body_limit(self) -> usize {
        self.max_body_bytes.unwrap_or(usize::MAX)
    }
}
