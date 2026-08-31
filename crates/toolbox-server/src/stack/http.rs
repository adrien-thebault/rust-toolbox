//! The standard stack for an HTTP API.

use tower::{Layer, ServiceBuilder};
use tower_http::{
    catch_panic::{CatchPanic, CatchPanicLayer, DefaultResponseForPanic},
    classify::{ServerErrorsAsFailures, SharedClassifier},
    trace::{Trace, TraceLayer},
};

use super::StackConfig;
use crate::{
    deadline::{DeadlineLayer, DeadlineService},
    trace_context::{MakeTracedSpan, TraceContextLayer, TraceContextService},
};

/// The service an HTTP stack wraps a router in.
pub type HttpStacked<S> = CatchPanic<
    TraceContextService<
        Trace<DeadlineService<S>, SharedClassifier<ServerErrorsAsFailures>, MakeTracedSpan>,
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

impl<S> Layer<S> for HttpStack {
    type Service = HttpStacked<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ServiceBuilder::new()
            .layer(CatchPanicLayer::new())
            .layer(TraceContextLayer::new())
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(MakeTracedSpan::new(self.cfg.trace_level)),
            )
            .layer(DeadlineLayer::new(self.cfg.timeout))
            .service(inner)
    }
}

/// The standard HTTP stack. See [`HttpStack`].
///
/// # Arguments
///
/// * `cfg` - Timeout, body limit and trace level for this server.
#[must_use]
pub fn http_stack(cfg: StackConfig) -> HttpStack {
    HttpStack { cfg }
}
