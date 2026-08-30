//! The stack for long-lived streams.

use tower::{Layer, ServiceBuilder};
use tower_http::{
    catch_panic::{CatchPanic, CatchPanicLayer, DefaultResponseForPanic},
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{Trace, TraceLayer},
};
use tracing::Level;

use crate::{
    span::MakeRequestSpan,
    trace_context::{TraceContextLayer, TraceContextService},
};

/// The service a realtime stack wraps a router in: no deadline, no body limit.
pub type RealtimeStacked<S> = CatchPanic<
    TraceContextService<Trace<S, SharedClassifier<ServerErrorsAsFailures>, MakeRequestSpan>>,
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

impl<S> Layer<S> for RealtimeStack {
    type Service = RealtimeStacked<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ServiceBuilder::new()
            .layer(CatchPanicLayer::new())
            .layer(TraceContextLayer::new())
            .layer(
                TraceLayer::new_for_http().make_span_with(MakeRequestSpan::new(self.trace_level)),
            )
            .service(inner)
    }
}

/// The stack for long-lived streams. See [`RealtimeStack`].
#[must_use]
pub fn realtime_stack() -> RealtimeStack {
    RealtimeStack {
        trace_level: Level::INFO,
    }
}
