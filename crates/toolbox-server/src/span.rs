//! The request span, built from the trace context rather than from a fresh id.
//!
//! It bridges `tower-http`'s tracing layer and this crate's W3C trace context,
//! which do not know about each other.

use http::Request;
use tower_http::trace::MakeSpan;
use tracing::{Level, Span};

use crate::trace_context::TraceContext;

/// Builds one span per request carrying method, path and the W3C trace id.
///
/// The trace id is the same string the response's `x-request-id` carries, so a
/// user quoting it from an error page lands on exactly these log lines.
#[derive(Debug, Clone, Copy)]
pub struct MakeRequestSpan {
    level: Level,
}

impl MakeRequestSpan {
    /// A span maker recording at `level`.
    ///
    /// # Arguments
    ///
    /// * `level` - What level to record request spans at. DEBUG for a chatty
    ///   internal service, INFO at an edge.
    #[must_use]
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl Default for MakeRequestSpan {
    fn default() -> Self {
        Self { level: Level::INFO }
    }
}

impl<B> MakeSpan<B> for MakeRequestSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let trace_id = request
            .extensions()
            .get::<TraceContext>()
            .map_or_else(String::new, |c| c.trace_id().to_owned());

        macro_rules! span {
            ($level:expr) => {
                tracing::span!(
                    $level,
                    "request",
                    method = %request.method(),
                    path = %request.uri().path(),
                    trace_id = %trace_id,
                    status = tracing::field::Empty,
                )
            };
        }

        match self.level {
            Level::TRACE => span!(Level::TRACE),
            Level::DEBUG => span!(Level::DEBUG),
            Level::WARN => span!(Level::WARN),
            Level::ERROR => span!(Level::ERROR),
            Level::INFO => span!(Level::INFO),
        }
    }
}
