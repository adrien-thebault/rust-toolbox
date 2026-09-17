//! The stack for long-lived streams.

use tower::{Layer, ServiceBuilder};
use tower_http::{
    catch_panic::{CatchPanic, CatchPanicLayer, DefaultResponseForPanic},
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{Trace, TraceLayer},
};
use tracing::Level;

use crate::trace_context::{MakeTracedSpan, TraceContextLayer, TraceContextService};

/// The service a realtime stack wraps a router in: no deadline, no body limit.
pub type RealtimeStacked<S> = CatchPanic<
    TraceContextService<Trace<S, SharedClassifier<ServerErrorsAsFailures>, MakeTracedSpan>>,
    DefaultResponseForPanic,
>;

/// The stack for long-lived streams.
///
/// It has **no timeout and no body limit**, and that is the entire reason it
/// exists: a 30-second request timeout silently kills every SSE and WebSocket
/// connection in production while working perfectly against a local client
/// that reconnects instantly.
#[derive(Debug, Clone, Copy)]
pub struct RealtimeStack {
    /// The level a dropped long-lived connection is logged at.
    trace_level: Level,
}

impl RealtimeStack {
    /// Build the long-lived HTTP middleware stack.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            trace_level: Level::INFO,
        }
    }

    /// Set the level used for request spans.
    #[must_use]
    pub const fn trace_level(mut self, trace_level: Level) -> Self {
        self.trace_level = trace_level;
        self
    }
}

impl Default for RealtimeStack {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for RealtimeStack {
    type Service = RealtimeStacked<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ServiceBuilder::new()
            .layer(CatchPanicLayer::new())
            .layer(TraceContextLayer::new())
            .layer(TraceLayer::new_for_http().make_span_with(MakeTracedSpan::new(self.trace_level)))
            .service(inner)
    }
}
