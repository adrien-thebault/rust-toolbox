//! The standard stack for an HTTP API.

use tower::{Layer, ServiceBuilder};
use tower_http::{
    catch_panic::{CatchPanic, CatchPanicLayer, DefaultResponseForPanic},
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{DefaultOnBodyChunk, DefaultOnFailure, DefaultOnRequest, Trace, TraceLayer},
};

use super::StackConfig;
use crate::{
    deadline::{DeadlineLayer, DeadlineService},
    trace_context::{MakeTracedSpan, TraceContextLayer, TraceContextService},
};

/// The service an HTTP stack wraps a router in.
pub type HttpStacked<S> = CatchPanic<
    TraceContextService<
        Trace<
            DeadlineService<S>,
            SharedClassifier<ServerErrorsAsFailures>,
            MakeTracedSpan,
            DefaultOnRequest,
            MakeTracedSpan,
            DefaultOnBodyChunk,
            MakeTracedSpan,
            DefaultOnFailure,
        >,
    >,
    DefaultResponseForPanic,
>;

/// The standard stack for an HTTP API.
///
/// Order, outermost first: catch-panic, trace context, request span, deadline.
/// Panic catching is outermost so a panic anywhere below it still produces a
/// response; the trace context is next so every span and every error body
/// carries the request id.
///
/// The body limit is not here - see [`StackConfig::max_body_bytes`].
#[derive(Debug, Clone, Copy)]
pub struct HttpStack {
    /// Timeouts, trace level and the rest, shared with the other stacks.
    cfg: StackConfig,
}

impl HttpStack {
    /// Build the HTTP middleware stack.
    #[must_use]
    pub const fn new(cfg: StackConfig) -> Self {
        Self { cfg }
    }
}

impl<S> Layer<S> for HttpStack {
    type Service = HttpStacked<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let traced = MakeTracedSpan::new(self.cfg.trace_level);
        ServiceBuilder::new()
            .layer(CatchPanicLayer::new())
            .layer(TraceContextLayer::new())
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(traced)
                    .on_response(traced)
                    .on_eos(traced),
            )
            .layer(DeadlineLayer::new(self.cfg.timeout))
            .service(inner)
    }
}
